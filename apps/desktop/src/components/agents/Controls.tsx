"use client";

// Model, thinking and usage live in the chat box, under what you type: the
// same place for a new session, a running one and the board. Each is a chip
// that opens a small popover above it, so no slash commands are needed.

import { type CSSProperties, type ReactNode, useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { type Session, seenUsage, seenWindows } from "@/lib/agents";
import { CLAUDE_MODELS, CODEX_MODELS, FAST_CLAUDE_MODEL } from "@/lib/ai-models";
import { setOverlayHit, useDuskie } from "@/lib/store";
import { Select, type SelectOption } from "../settings/ui";
import { AGENT_IMAGES, AGENT_MARKS } from "./AgentsTab";

/** Thinking levels; "max" is the most each agent allows. */
const LEVELS: [string, string, string][] = [
  ["low", "Low", "Quick answers, least usage."],
  ["medium", "Medium", "A good default for most changes."],
  ["high", "High", "Plans first. Slower, uses more."],
  ["max", "Max", "For hard bugs. Uses the most."],
];

/** One line on each model, and how much it can hold. */
const ABOUT: Record<string, [string, string]> = {
  [FAST_CLAUDE_MODEL]: ["Fast, light tasks", "200k"],
  "claude-opus-5-5": ["Most capable, uses limits fastest", "1M"],
  "claude-sonnet-5-5": ["Everyday coding, balanced", "1M"],
  "claude-fable-5-1": ["Long, careful work", "1M"],
  "claude-fable-5": ["Long, careful work", "1M"],
  "claude-opus-4-6": ["Previous Opus", "200k"],
  "claude-sonnet-4-6": ["Previous Sonnet", "200k"],
};

const idOf = (agent: string) => agent.toLowerCase().replace(/\s+/g, "_");
export const markOf = (agent: string): [string, string] => AGENT_MARKS[idOf(agent)] ?? ["?", "#2b2b30"];

/** The models an agent can switch between here; none when it picks its own. */
export function modelsFor(agent: string): [string, string][] {
  const id = idOf(agent);
  if (id === "claude_code") return CLAUDE_MODELS;
  if (id === "codex") return CODEX_MODELS;
  return [];
}

const Caret = () => (
  <svg viewBox="0 0 16 16" aria-hidden="true" className="size-3 opacity-60">
    <path d="m4 6 4 4 4-4" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
  </svg>
);

const Brain = () => (
  <svg viewBox="0 0 16 16" aria-hidden="true" className="size-3.5 opacity-75">
    <path
      d="M6 3a2 2 0 0 0-2 2 2 2 0 0 0-1 3.5A2 2 0 0 0 5 12a2 2 0 0 0 3 .5V3.5A2 2 0 0 0 6 3Zm4 0a2 2 0 0 1 2 2 2 2 0 0 1 1 3.5 2 2 0 0 1-2 3.5 2 2 0 0 1-3 .5"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.2"
      strokeLinejoin="round"
    />
  </svg>
);

/** A chip that opens a popover above itself (portaled so the island shell
 *  does not clip it; same pattern as the model Select overlay). */
function Pop({
  label,
  chip,
  className = "",
  children,
  requested,
}: {
  label: string;
  chip: ReactNode;
  className?: string;
  children: (close: () => void) => ReactNode;
  requested?: number;
}) {
  const [open, setOpen] = useState(false);
  useEffect(() => {
    if (requested) setOpen(true);
  }, [requested]);
  const btn = useRef<HTMLButtonElement>(null);
  const box = useRef<HTMLDivElement>(null);
  const islandColor = useDuskie((s) => s.settings.islandColor);
  const [at, setAt] = useState<{ left: number; bottom: number } | null>(null);
  useEffect(() => {
    if (!open) {
      setAt(null);
      return;
    }
    const r = btn.current?.getBoundingClientRect();
    // Grows from the chip: left-aligned on the left half, right-aligned on the right.
    if (r)
      setAt({
        left: r.left + r.width / 2 > window.innerWidth / 2 ? r.right - 324 : r.left,
        bottom: window.innerHeight - r.top + 8,
      });
    const onDoc = (e: MouseEvent) => {
      const t = e.target as Node;
      if (!btn.current?.contains(t) && !box.current?.contains(t)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        setOpen(false);
        btn.current?.focus();
      }
    };
    document.addEventListener("mousedown", onDoc);
    document.addEventListener("keydown", onKey, true);
    return () => {
      document.removeEventListener("mousedown", onDoc);
      document.removeEventListener("keydown", onKey, true);
      setOverlayHit(null);
    };
  }, [open]);
  // Keep the popover on screen and clickable above the island.
  useLayoutEffect(() => {
    const el = box.current;
    if (!open || !at || !el) return;
    const r = el.getBoundingClientRect();
    const left = Math.max(8, Math.min(at.left, window.innerWidth - r.width - 8));
    if (left !== at.left) setAt({ ...at, left });
    setOverlayHit({ x: left, y: r.top, width: r.width, height: r.height });
  }, [open, at]);
  return (
    <>
      <button
        ref={btn}
        type="button"
        aria-label={label}
        aria-expanded={open}
        aria-haspopup="dialog"
        onClick={() => setOpen((o) => !o)}
        className={`ak-cchip chip ${className}`}
      >
        {chip}
      </button>
      {open &&
        at &&
        createPortal(
          <div
            ref={box}
            role="dialog"
            aria-label={label}
            data-color={islandColor}
            className="ak-pop menu"
            style={{ left: at.left, bottom: at.bottom, zIndex: 100 }}
          >
            {children(() => setOpen(false))}
          </div>,
          document.body,
        )}
    </>
  );
}

function windowName(w: string): string {
  if (w === "five_hour") return "5-hour";
  if (w.startsWith("seven_day")) return "Weekly";
  return w;
}

function resetText(at: number | null): string {
  if (!at) return "";
  const ms = at * 1000 - Date.now();
  if (ms <= 0) return "Reset";
  const h = Math.floor(ms / 3_600_000);
  const m = Math.round((ms % 3_600_000) / 60_000);
  if (h >= 24)
    return `Resets ${new Date(at * 1000).toLocaleString([], { weekday: "short", hour: "numeric", minute: "2-digit" })}`;
  return h ? `Resets in ${h} hr ${m} min` : `Resets in ${m} min`;
}

function ago(t: number): string {
  const mins = Math.round((Date.now() - t) / 60_000);
  return mins < 1 ? "just now" : mins < 60 ? `${mins} min ago` : `${Math.round(mins / 60)} h ago`;
}

/** Ticks only while the usage popover is open (this component mounts with the pop). */
function UsageBody({
  usage,
  ctxPct,
  plans,
  now,
  agent,
  onCompact,
  onRefresh,
}: {
  usage?: Session["usage"];
  ctxPct: number | null;
  plans: { used: number; window: string; resetsAt: number | null; at: number }[];
  now: number;
  agent: string;
  onCompact?: () => void;
  onRefresh?: () => void;
}) {
  const [, tick] = useState(0);
  useEffect(() => {
    const timer = setInterval(() => tick((n) => n + 1), 1000);
    return () => clearInterval(timer);
  }, []);
  return (
    <>
      {!usage && <p className="ak-note">Context capacity: Not reported</p>}
      {usage && ctxPct !== null && (
        <>
          <div className="ak-pop-h">
            <span>Context window</span>
            <span className="ak-pop-m">
              {Math.round(usage.used / 1000)}k / {Math.round(usage.window / 1000)}k ({ctxPct}%)
            </span>
          </div>
          <div className="ak-pbar">
            <i className="ak-pfill" style={{ width: `${ctxPct}%` }} />
          </div>
          <p className="ak-note">
            {Math.max(0, usage.window - usage.used).toLocaleString()} tokens remaining {"\u00b7"} {100 - ctxPct}%
            remaining
          </p>
          <p className="ak-note">
            Latest usage: {usage.used.toLocaleString()} tokens. Session total:{" "}
            {usage.total?.toLocaleString() ?? "Not reported"}. {usage.at ? `Reported ${ago(usage.at)}` : "Cached"}
          </p>
        </>
      )}
      {usage && plans.length > 0 && <div className="ak-pop-sep" />}
      {plans.length > 0 && (
        <>
          <div className="ak-pop-h">
            <span>Account allowance</span>
          </div>
          {plans.map((p) => {
            const pPct = Math.round(p.used <= 1 ? p.used * 100 : p.used);
            return (
              <div key={p.window} className="ak-lim">
                <div className="ak-pop-h">
                  <span className="font-normal">{windowName(p.window)}</span>
                  <span className="ak-pop-m">
                    {`${100 - pPct}% remaining \u00b7 ${resetText(p.resetsAt) || "Reset not reported"}`}
                  </span>
                </div>
                <p className="ak-note">
                  {p.resetsAt ? new Date(p.resetsAt * 1000).toLocaleString() : "Reset timestamp: Not reported"}{" "}
                  {"\u00b7"}{" "}
                  {p.resetsAt && p.resetsAt * 1000 <= now ? "Expired; refresh required" : `Cached report ${ago(p.at)}`}
                </p>
                <div className="ak-pbar">
                  <i
                    className="ak-pfill"
                    style={{ width: `${pPct}%`, background: pPct > 65 ? "#ff9f0a" : undefined }}
                  />
                </div>
              </div>
            );
          })}
        </>
      )}
      <div className="ak-pop-h">
        <span className="ak-pop-m">
          {idOf(agent) === "codex" && plans.length === 0
            ? "Account allowance: Not reported (API-key accounts may not report this)"
            : plans.length > 0
              ? `Sent with ${agent}'s usage warnings; last seen ${ago(Math.max(...plans.map((p) => p.at)))}`
              : "Plan limits show once the agent reports them"}
        </span>
        {onCompact && (
          <button type="button" className="chip ak-pop-btn" onClick={onCompact}>
            Compact
          </button>
        )}
        {onRefresh && (
          <button type="button" className="chip ak-pop-btn" onClick={onRefresh}>
            Refresh allowance
          </button>
        )}
      </div>
      <p className="ak-note">
        Context capacity is what one turn can hold. Account allowance limits service usage over a time window.
        Compaction summarizes earlier messages to free context; it does not reset allowance.
      </p>
    </>
  );
}

export function ChatControls({
  agent,
  model,
  effort,
  onModel,
  onEffort,
  usage,
  limit,
  onCompact,
  keys,
  localModel,
  panel,
  onRefresh,
}: {
  agent: string;
  model: string | null | undefined;
  effort: string | null | undefined;
  onModel: (m: string | null) => void;
  onEffort: (e: string | null) => void;
  usage?: Session["usage"];
  limit?: Session["limit"];
  onCompact?: () => void;
  /** The key hint at the end ("Enter send"). */
  keys?: string;
  /** For Local: the Ollama model it runs. */
  localModel?: string;
  panel?: { name: string; at: number };
  onRefresh?: () => void;
}) {
  const models = modelsFor(agent);
  const [mark, color] = markOf(agent);
  const local = idOf(agent) === "local";
  const level = LEVELS.find(([v]) => v === effort);

  // Plan windows: this session's warning plus every window Duskie has seen.
  const now = Date.now();
  const byWindow = new Map<string, { used: number; window: string; resetsAt: number | null; at: number }>();
  for (const u of seenWindows(agent)) {
    byWindow.set(u.window, u);
  }
  if (limit && limit.used !== null) {
    byWindow.set(limit.window, {
      used: limit.used,
      window: limit.window,
      resetsAt: limit.resetsAt,
      at: now,
    });
  }
  const plans = [...byWindow.values()].sort((a, b) => {
    const rank = (w: string) => (w === "five_hour" ? 0 : w.startsWith("seven_day") ? 1 : 2);
    return rank(a.window) - rank(b.window);
  });
  const top = seenUsage(agent);
  const planPct = top ? Math.round(top.used <= 1 ? top.used * 100 : top.used) : null;
  const ctxPct = usage && usage.window > 0 ? Math.round(Math.min(1, usage.used / usage.window) * 100) : null;
  const ringPct = ctxPct ?? planPct ?? 0;
  const usageText = [
    ctxPct !== null ? `${ctxPct}% context` : null,
    top && planPct !== null ? `${planPct}% ${windowName(top.window).toLowerCase()}` : null,
  ]
    .filter(Boolean)
    .join(" \u00b7 ");

  const image = AGENT_IMAGES[idOf(agent)];
  const modelOptions: SelectOption[] = [
    {
      value: "",
      label: "Default",
      sub: "What the agent is set to use",
      icon: image ? undefined : mark,
      image,
      color,
    },
    ...models.map(([v, l]) => ({
      value: v,
      label: l,
      sub: ABOUT[v] ? `${ABOUT[v][0]} \u00b7 ${ABOUT[v][1]}` : undefined,
      icon: image ? undefined : mark,
      image,
      color,
    })),
  ];
  const modelName = models.find(([v]) => v === model)?.[1] ?? "Default model";

  return (
    <div className="ak-ctl">
      {panel?.name === "model" && (
        <Pop label="Choose model" requested={panel.at} chip="Choose model">
          {(close) => (
            <>
              {[["", "Default"], ...models].map(([value, label]) => (
                <button
                  type="button"
                  className="chip ak-pop-btn"
                  key={value}
                  onClick={() => {
                    onModel(value || null);
                    close();
                  }}
                >
                  {label}
                </button>
              ))}
            </>
          )}
        </Pop>
      )}
      {local ? (
        <span className="ak-cchip">
          <i className="ak-cdot" style={{ background: "#30d158" }} />
          {localModel ?? "Local model"}
        </span>
      ) : (
        models.length > 0 && (
          <span className="ak-cmodel">
            <i className="ak-cdot" style={{ background: color }} aria-hidden="true" />
            <Select
              variant="plain"
              overlay
              label="Model"
              menuWidth={324}
              group={`${agent} models, from your next message`}
              current={modelName}
              value={model ?? ""}
              onChange={(v) => onModel(v || null)}
              options={modelOptions}
            />
          </span>
        )
      )}
      {(models.length > 0 || local) && (
        <Pop
          label="Thinking"
          requested={panel?.name === "effort" ? panel.at : undefined}
          chip={
            <>
              <Brain />
              {local ? (effort === "off" ? "Off" : "On") : (level?.[1] ?? "Default")}
              <Caret />
            </>
          }
        >
          {(close) =>
            local ? (
              <>
                <div className="ak-pop-h">
                  <span>Thinking</span>
                </div>
                <fieldset className="ak-tseg" aria-label="Thinking">
                  {[
                    ["", "On"],
                    ["off", "Off"],
                  ].map(([v, l]) => (
                    <button
                      key={v}
                      type="button"
                      aria-pressed={(effort ?? "") === v}
                      onClick={() => {
                        onEffort(v || null);
                        close();
                      }}
                    >
                      {l}
                    </button>
                  ))}
                </fieldset>
                <p className="ak-pop-m">For models that think. Off answers faster.</p>
              </>
            ) : (
              <>
                <div className="ak-pop-h">
                  <span>How hard it thinks</span>
                  <span className="ak-dots" aria-hidden="true">
                    {LEVELS.map(([v], i) => (
                      <i key={v} className="ak-dot" data-on={level ? i <= LEVELS.indexOf(level) : false} />
                    ))}
                  </span>
                </div>
                <fieldset className="ak-tseg" aria-label="How hard it thinks">
                  <button type="button" aria-pressed={!level} onClick={() => onEffort(null)}>
                    Default
                  </button>
                  {LEVELS.map(([v, l]) => (
                    <button key={v} type="button" aria-pressed={effort === v} onClick={() => onEffort(v)}>
                      {l}
                    </button>
                  ))}
                </fieldset>
                <p className="ak-pop-m">{level ? level[2] : "The agent's own default."}</p>
              </>
            )
          }
        </Pop>
      )}
      <span className="flex-1" />
      {local ? (
        <span className="ak-cchip ak-cquiet">
          <svg viewBox="0 0 16 16" aria-hidden="true" className="size-3">
            <rect x="3.5" y="7" width="9" height="6.5" rx="1.5" fill="none" stroke="currentColor" strokeWidth="1.3" />
            <path d="M5.5 7V5a2.5 2.5 0 0 1 5 0v2" fill="none" stroke="currentColor" strokeWidth="1.3" />
          </svg>
          On this PC {"\u00b7"} no limits
        </span>
      ) : (
        (usageText || panel?.name === "usage") && (
          <Pop
            label="Usage"
            requested={panel?.name === "usage" ? panel.at : undefined}
            className="ak-cusage"
            chip={
              <>
                <span className="ak-ring" aria-hidden="true">
                  <i className="ak-cring" style={{ "--p": ringPct } as CSSProperties} />
                </span>
                {usageText || "Usage"}
              </>
            }
          >
            {() => (
              <UsageBody
                usage={usage}
                ctxPct={ctxPct}
                plans={plans}
                now={now}
                agent={agent}
                onCompact={onCompact}
                onRefresh={onRefresh}
              />
            )}
          </Pop>
        )
      )}
      {keys && <span className="ckeys mono">{keys}</span>}
    </div>
  );
}
