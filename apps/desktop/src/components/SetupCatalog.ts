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
    why: "Chat, drafts and skills with your Claude plan. Run claude once afterwards to sign in.",
    recommended: true,
  },
  {
    id: "codex",
    group: "ai",
    title: "Codex",
    why: "OpenAI's coding agent, with your ChatGPT plan. Use it instead of Claude Code or next to it. Run codex once afterwards to sign in.",
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
    why: "Answers when Claude is not set up, without anything leaving the PC. About 2.5 GB.",
    recommended: false,
  },
  {
    id: "ollama_embed",
    group: "ai",
    title: "Search model",
    why: "Finds things by meaning, not only exact words. About 270 MB.",
    recommended: true,
  },
  {
    id: "ollama_vision",
    group: "ai",
    title: "Vision model",
    why: "Sees pictures on your screen that have no text. About 1.7 GB, loaded only when needed.",
    recommended: false,
  },
  {
    id: "anthropic",
    group: "ai",
    title: "Anthropic API key",
    why: "Pay as you go instead of a Claude plan. Restart Sidekick after setting it.",
    recommended: false,
  },
  {
    id: "claude_hooks",
    group: "connect",
    title: "Claude Code hooks",
    why: "Know when a session finishes or waits, and allow or deny its requests from the island. Add for me merges into Claude Code settings (backed up first).",
    recommended: true,
  },
  {
    id: "claude_mcp",
    group: "connect",
    title: "Sidekick tools in Claude Code",
    why: "Lets Claude Code search your history, notify you and open links.",
    recommended: false,
  },
  {
    id: "codex_notify",
    group: "connect",
    title: "Codex notifications",
    why: "Know when a Codex turn is done. Add for me adds Sidekick to Codex settings (backed up first).",
    recommended: false,
  },
  {
    id: "codex_mcp",
    group: "connect",
    title: "Sidekick tools in Codex",
    why: "Lets Codex search your history, notify you and open links.",
    recommended: false,
  },
  {
    id: "browser",
    group: "connect",
    title: "Browser extension",
    why: "Page summaries, form help, duplicate tabs and saving sessions.",
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
    why: "Connect once in the browser. Your calendar, Gmail, Slack, Jira and more then work in Sidekick.",
    recommended: true,
  },
  {
    id: "calendar",
    group: "connect",
    title: "Calendar",
    why: "Meeting reminders with Join and Prep. Connect Google Calendar or Outlook on Composio.",
    recommended: false,
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
    id: "fathom",
    group: "connect",
    title: "Fathom",
    why: "Follow-ups drafted from your meeting notes. Connect Fathom on Composio.",
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
    why: "Opens your projects and files. VS Code, Cursor or Windsurf.",
    recommended: true,
  },
  {
    id: "tesseract",
    group: "tools",
    title: "Tesseract",
    why: "Reads text in screenshots, so Copy text works on a screenshot of an error or a page.",
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
  {
    id: "passwords",
    group: "tools",
    title: "Password manager CLI",
    why: "Fill logins from Bitwarden (or 1Password with op).",
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
    tab: null,
    recommended: entry.recommended,
  };
}
