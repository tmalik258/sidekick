// Versioned CLI model choices verified against provider documentation on 2026-10-03.
// https://learn.chatgpt.com/docs/models
// https://code.claude.com/docs/en/model-config
export const FAST_CODEX_MODEL = "gpt-6-luna";
export const FAST_CLAUDE_MODEL = "claude-haiku-4-5-20251001";

export const CODEX_MODELS: [string, string][] = [
  [FAST_CODEX_MODEL, "GPT-6 Luna (fast)"],
  ["gpt-6.1-sol", "GPT-6.1 Sol"],
  ["gpt-6-sol", "GPT-6 Sol"],
  ["gpt-6-astra", "GPT-6 Astra"],
  ["gpt-5.6-luna", "GPT-5.6 Luna"],
  ["gpt-5.6-sol", "GPT-5.6 Sol"],
  ["gpt-5.6-terra", "GPT-5.6 Terra"],
];

export const CLAUDE_MODELS: [string, string][] = [
  [FAST_CLAUDE_MODEL, "Haiku 4.5 (fast)"],
  ["claude-sonnet-5-5", "Sonnet 5.5"],
  ["claude-opus-5-5", "Opus 5.5"],
  ["claude-fable-5-1", "Fable 5.1"],
  ["claude-fable-5", "Fable 5"],
  ["claude-sonnet-4-6", "Sonnet 4.6"],
  ["claude-opus-4-6", "Opus 4.6"],
];

export function explicitModel(value: string, fastModel: string): string {
  const model = value.trim();
  return model && model !== "default" ? model : fastModel;
}
