/** Built-in catalog validated with Codex CLI 0.162.1. */
export const CODEX_COMMAND_VERSION = "0.162.1";
export interface AgentCommand {
  name: string;
  description: string;
  group: string;
  execution: "control" | "provider" | "terminal";
}
const controls: [string, string][] = [
  ["plan", "Plan the next message"],
  ["ask", "Ask before changes"],
  ["edit", "Allow workspace edits"],
  ["full", "Allow full access"],
  ["permissions", "Choose permissions"],
  ["model", "Choose model"],
  ["effort", "Choose reasoning effort"],
  ["status", "Show session status"],
  ["usage", "Context and account allowance"],
  ["compact", "Compact context"],
  ["diff", "Review file changes"],
  ["clear", "Start a fresh session in this project"],
  ["new", "Open a new session"],
  ["rewind", "Rewind to an earlier message"],
  ["rename", "Rename this session"],
  ["copy", "Copy the latest response"],
  ["resume", "Resume this session"],
];
export const CONTROL_COMMANDS: AgentCommand[] = controls.map(([name, description]) => ({
  name: `/${name}`,
  description,
  group: "Duskie",
  execution: "control",
}));
export const TERMINAL_COMMANDS: AgentCommand[] = [
  "agent",
  "subagents",
  "apps",
  "approvals",
  "approve",
  "archive",
  "delete",
  "debug-config",
  "exit",
  "experimental",
  "feedback",
  "fork",
  "init",
  "logout",
  "mcp",
  "mention",
  "personality",
  "ps",
  "quit",
  "review",
  "sandbox-add-read-dir",
  "skills",
  "theme",
  "fast",
  "stop",
  "statusline",
  "ide",
  "keymap",
  "vim",
  "setup-default-sandbox",
  "plugins",
  "hooks",
  "memories",
  "import",
  "goal",
  "app",
  "side",
  "btw",
  "raw",
  "title",
  "pet",
  "pets",
].map((name) => ({
  name: `/${name}`,
  description: name === "review" ? "Request an agent code review · Terminal only" : "Terminal only",
  group: `Codex ${CODEX_COMMAND_VERSION} · Terminal only`,
  execution: "terminal",
}));

export function commandCatalog(provider: string, commands: AgentCommand[]): AgentCommand[] {
  const reserved = new Set(CONTROL_COMMANDS.map((c) => c.name));
  return [
    ...CONTROL_COMMANDS,
    ...commands.filter((c) => !reserved.has(c.name)),
    ...(provider === "Codex" ? TERMINAL_COMMANDS : []),
  ];
}
