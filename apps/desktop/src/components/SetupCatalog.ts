// Static setup rows so the checklist paints before setup_status returns.
// Titles mirror apps/desktop/src-tauri/src/setup.rs.

import type { SetupGroup, SetupItem } from "@/lib/types";

export const SETUP_CATALOG: Array<{
  id: string;
  group: SetupGroup;
  title: string;
  why: string;
  recommended: boolean;
}> = [
  {
    id: "claude_code",
    group: "ai",
    title: "Claude Code",
    why: "Uses your Claude plan. Run claude once to sign in.",
    recommended: true,
  },
  {
    id: "codex",
    group: "ai",
    title: "Codex",
    why: "Uses your ChatGPT plan. Run codex once to sign in.",
    recommended: false,
  },
  {
    id: "ollama",
    group: "ai",
    title: "Ollama",
    why: "Free local AI on this PC, and search by meaning.",
    recommended: true,
  },
  {
    id: "ollama_chat",
    group: "ai",
    title: "Local chat model",
    why: "Private answers on this PC, from a model that suits it.",
    recommended: false,
  },
  {
    id: "ollama_embed",
    group: "ai",
    title: "Search model",
    why: "Search by meaning. About 270 MB.",
    recommended: true,
  },
  {
    id: "ollama_vision",
    group: "ai",
    title: "Vision model",
    why: "Reads pictures without text. About 1.7 GB.",
    recommended: false,
  },
  {
    id: "ollama_light",
    group: "ai",
    title: "Lighter Ollama",
    why: "8K context with about half the memory, so answers keep the whole question. Restarts Ollama.",
    recommended: true,
  },
  {
    id: "anthropic",
    group: "ai",
    title: "Anthropic API key",
    why: "Pay as you go. Opens PowerShell with the command ready to paste.",
    recommended: false,
  },
  {
    id: "claude_hooks",
    group: "connect",
    title: "Claude Code hooks",
    why: "See when it finishes, allow or deny from the island.",
    recommended: true,
  },
  {
    id: "claude_mcp",
    group: "connect",
    title: "Sidekick tools in Claude Code",
    why: "Lets Claude Code use Sidekick's tools.",
    recommended: false,
  },
  {
    id: "codex_notify",
    group: "connect",
    title: "Codex notifications",
    why: "See when a Codex turn is done.",
    recommended: false,
  },
  {
    id: "codex_mcp",
    group: "connect",
    title: "Sidekick tools in Codex",
    why: "Lets Codex use Sidekick's tools.",
    recommended: false,
  },
  {
    id: "browser",
    group: "connect",
    title: "Browser extension",
    why: "Page help, tabs and sessions on the island.",
    recommended: true,
  },
  {
    id: "code_folders",
    group: "connect",
    title: "Code folders",
    why: "Project status, the project launcher and the end-of-day check.",
    recommended: true,
  },
  {
    id: "composio",
    group: "connect",
    title: "Composio",
    why: "One sign-in for your calendar, mail, Slack and more.",
    recommended: true,
  },
  {
    id: "search_folders",
    group: "connect",
    title: "Folders to search",
    why: "Search inside your documents and notes from Ask mode.",
    recommended: false,
  },
  {
    id: "voice",
    group: "connect",
    title: "Voice",
    why: 'Say "Hey Sidekick" and hear answers. About 205 MB, all on this PC.',
    recommended: false,
  },
  {
    id: "gh",
    group: "tools",
    title: "GitHub CLI",
    why: "Open PRs in the morning brief.",
    recommended: true,
  },
  {
    id: "git",
    group: "tools",
    title: "Git",
    why: "Repo status and unsaved work.",
    recommended: true,
  },
  {
    id: "vscode",
    group: "tools",
    title: "Code editor",
    why: "Opens your projects and files: Cursor, VS Code, Antigravity, PyCharm and others.",
    recommended: true,
  },
  {
    id: "tesseract",
    group: "tools",
    title: "Tesseract",
    why: "Copy text out of screenshots.",
    recommended: true,
  },
  {
    id: "poppler",
    group: "tools",
    title: "Poppler",
    why: "Summarize PDFs.",
    recommended: false,
  },
  {
    id: "pandoc",
    group: "tools",
    title: "Pandoc",
    why: "Summarize Word files and convert documents.",
    recommended: false,
  },
  {
    id: "ffmpeg",
    group: "tools",
    title: "FFmpeg",
    why: "Convert videos and audio.",
    recommended: false,
  },
  {
    id: "magick",
    group: "tools",
    title: "ImageMagick",
    why: "Convert and resize images.",
    recommended: false,
  },
  {
    id: "docker",
    group: "tools",
    title: "Docker Desktop",
    why: "Start Docker when a project needs it.",
    recommended: false,
  },
  {
    id: "libreoffice",
    group: "tools",
    title: "LibreOffice",
    why: "Convert Office files to PDF.",
    recommended: false,
  },
];

export function skeletonItem(entry: (typeof SETUP_CATALOG)[number]): SetupItem {
  return {
    id: entry.id,
    group: entry.group,
    title: entry.title,
    why: entry.why,
    done: false,
    status: "",
    command: null,
    runnable: false,
    action: "Install",
    opensApp: false,
    opensTerminal: false,
    tab: null,
    recommended: entry.recommended,
  };
}
