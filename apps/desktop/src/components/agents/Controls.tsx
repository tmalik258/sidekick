"use client";

// Model, thinking and usage live in the chat box, under what you type: the
// same place for a new session, a running one and the board.

import type { Session } from "@/lib/agents";
import { CLAUDE_MODELS, CODEX_MODELS } from "@/lib/ai-models";
import { Select } from "../settings/ui";
import { ContextRing } from "./AgentsTab";

const EFFORTS: [string, string][] = [
  ["", "Thinking: default"],
  ["off", "Thinking: off"],
  ["low", "Thinking: low"],
  ["medium", "Thinking: medium"],
  ["high", "Thinking: high"],
];

/** The models an agent can switch between here; none when it picks its own. */
export function modelsFor(agent: string): [string, string][] {
  if (agent === "Claude Code" || agent === "claude_code") return CLAUDE_MODELS;
  if (agent === "Codex" || agent === "codex") return CODEX_MODELS;
  return [];
}

function limitLine(l: NonNullable<Session["limit"]>): string | null {
  if (l.used === null) return null;
  const span = l.window === "five_hour" ? "5-hour" : l.window.startsWith("seven_day") ? "week" : "plan";
  return `${Math.round(l.used)}% of ${span}`;
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
}) {
  const models = modelsFor(agent);
  const thinks = models.length > 0;
  const plan = limit ? limitLine(limit) : null;
  return (
    <div className="ak-ctl">
      {models.length > 0 && (
        <span className="ak-mi">
          <Select
            variant="plain"
            overlay
            label="Model"
            value={model ?? ""}
            onChange={(v) => onModel(v || null)}
            options={[["", "Model: default"], ...models]}
          />
        </span>
      )}
      {thinks && (
        <span className="ak-mi">
          <Select
            variant="plain"
            overlay
            label="Thinking"
            value={effort ?? ""}
            onChange={(v) => onEffort(v || null)}
            options={EFFORTS}
          />
        </span>
      )}
      <span className="flex-1" />
      {plan && <span className="ak-mi">{plan}</span>}
      {usage && <ContextRing used={usage.used} window={usage.window} onCompact={onCompact} />}
      {keys && <span className="ckeys mono">{keys}</span>}
    </div>
  );
}
