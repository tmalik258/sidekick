"use client";

// One setup checklist row: status, Run / Add for me / Set up, and inline guides.

import { useState } from "react";
import { api } from "@/lib/bridge";
import type { SetupItem } from "@/lib/types";
import { ItemGuide } from "./SetupGuides";

const TAB_LABELS: Record<string, string> = {
  ai: "AI",
  browser: "Browser",
  general: "General",
  search: "Search",
  today: "Today",
  voice: "Voice",
  connections: "Connections",
};

const ONE_CLICK = new Set([
  "claude_hooks",
  "claude_mcp",
  "browser",
  "code_folders",
  "search_folders",
  "voice",
  "composio",
  "calendar",
  "fathom",
]);

function oneClickLabel(id: string): string {
  if (id === "claude_hooks" || id === "claude_mcp") return "Add for me";
  if (id === "composio" || id === "calendar" || id === "fathom") return "Connect";
  return "Set up";
}

export function SetupRow({
  item,
  onRun,
  onOpenTab,
  inlineGuides,
  guideOpen,
  onToggleGuide,
  onDone,
  checking = false,
}: {
  item: SetupItem;
  onRun: (id: string) => void;
  onOpenTab?: (tab: string) => void;
  inlineGuides?: boolean;
  guideOpen: boolean;
  onToggleGuide: () => void;
  onDone: () => void;
  /** Pulse glyph while probes are in flight. */
  checking?: boolean;
}) {
  const [busy, setBusy] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const multiline = item.command?.includes("\n") ?? false;
  const oneClick = ONE_CLICK.has(item.id);
  const needsGuide = (Boolean(item.tab) && !item.runnable) || oneClick;
  const showInline = Boolean(inlineGuides && needsGuide);
  const showOpenTab = Boolean(!inlineGuides && needsGuide && onOpenTab && item.tab);
  const showRun = Boolean(item.runnable && item.id !== "claude_hooks");

  const runDirect = () => {
    if (item.id === "browser" || item.id === "code_folders" || item.id === "search_folders" || item.id === "voice") {
      onToggleGuide();
      return;
    }
    setBusy(true);
    setActionError(null);
    const work =
      item.id === "claude_hooks"
        ? api.claudeAddHooks().then(() => undefined)
        : item.id === "claude_mcp"
          ? api.claudeAddMcp().catch(() => api.setupRun("claude_mcp"))
          : api.composioSignIn().then(() => undefined);
    void work
      .then(onDone)
      .catch((e) => setActionError(String(e)))
      .finally(() => setBusy(false));
  };

  const isDirect =
    item.id === "claude_hooks" ||
    item.id === "claude_mcp" ||
    item.id === "composio" ||
    item.id === "calendar" ||
    item.id === "fathom";

  return (
    <div className="flex flex-col gap-1.5 rounded-2xl bg-white/[0.06] px-3.5 py-2.5">
      <div className="flex items-center gap-3">
        {checking ? (
          <span
            role="img"
            aria-label="Checking"
            className="size-[18px] shrink-0 animate-pulse rounded-full bg-amber-400/90"
          />
        ) : (
          <span
            role="img"
            aria-label={item.done ? "Done" : "Not done"}
            className={`grid size-[18px] shrink-0 place-items-center rounded-full text-[10px] font-bold ${
              item.done ? "bg-[#30d158] text-black" : "ring-1 ring-white/30 ring-inset"
            }`}
          >
            {item.done ? "✓" : ""}
          </span>
        )}
        <div className="min-w-0 flex-1">
          <p className="font-medium text-white">
            {item.title}{" "}
            {!checking && (
              <span className={`font-normal ${item.done ? "text-[#30d158]" : "text-[rgb(235_235_245/0.5)]"}`}>
                {item.status}
              </span>
            )}
          </p>
          <p className="text-[11.5px] leading-snug text-[rgb(235_235_245/0.55)]">{item.why}</p>
        </div>
        {!checking && !item.done && (
          <div className="flex shrink-0 gap-1.5">
            {item.command && !oneClick && <CopyButton text={item.command} />}
            {showRun && (
              <SmallButton primary={!showInline && !isDirect} onClick={() => onRun(item.id)}>
                Run
              </SmallButton>
            )}
            {showInline && (
              <SmallButton
                primary
                disabled={busy}
                onClick={() => {
                  if (isDirect) runDirect();
                  else onToggleGuide();
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
            {!showInline && !isDirect && needsGuide && item.id === "browser" && onOpenTab && (
              <SmallButton primary onClick={() => onOpenTab("browser")}>
                Open Browser
              </SmallButton>
            )}
          </div>
        )}
      </div>
      {actionError && <p className="text-[11px] text-[#ff453a]">{actionError}</p>}
      {!checking && !item.done && item.command && !multiline && !oneClick && <Code text={item.command} />}
      {!checking && !item.done && showInline && guideOpen && <ItemGuide id={item.id} onDone={onDone} />}
    </div>
  );
}

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
  children: string;
  onClick: () => void;
  disabled?: boolean;
  primary?: boolean;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      className={`chip rounded-full px-2.5 py-1 text-[12px] font-medium disabled:opacity-50 ${
        primary ? "bg-white text-black hover:bg-white/90" : "bg-white/12 text-white/90 hover:bg-white/20"
      }`}
    >
      {children}
    </button>
  );
}
