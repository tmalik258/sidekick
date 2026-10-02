// Browser-only stand-in for the Rust core, used when the UI runs outside
// Tauri. It mimics the demo flow loosely; the real rules live in Rust.

import {
  CUES,
  type Cue,
  DEFAULT_SETTINGS,
  type MascotState,
  type Settings,
  type Suggestion,
  type WelcomeSpeech,
} from "./types";

type Handler = (payload: unknown) => void;

const handlers = new Map<string, Set<Handler>>();
let settings: Settings = structuredClone(DEFAULT_SETTINGS);
let mascot: MascotState = "idle";
let suggestion: Suggestion | null = null;
let epoch = 0;

const CUE_BY_STATE: Partial<Record<MascotState, Cue>> = {
  idle: "settle",
  sleeping: "yawn",
  noticing: "chirp",
  suggesting: "pop",
  listening: "open",
  success: "ding",
  error: "boop",
};

function emit(event: string, payload: unknown) {
  for (const h of handlers.get(event) ?? []) h(payload);
}

function go(state: MascotState) {
  const previous = mascot;
  mascot = state;
  epoch += 1;
  emit("mascot://state", { previous, state, cue: CUE_BY_STATE[state] ?? null });
  if (state === "success") later(1500, () => go("idle"));
  if (state === "error") later(4000, () => go("idle"));
}

function later(ms: number, fn: () => void) {
  const at = epoch;
  setTimeout(() => {
    if (epoch === at) fn();
  }, ms);
}

function saveSettings(next: Settings): Settings {
  settings = next;
  emit("settings://changed", settings);
  emit("voice://state", commands.voice_status?.({}));
  return settings;
}

const commands: Record<string, (args: Record<string, unknown>) => unknown> = {
  app_info: () => ({ version: "0.1.0 (browser mock)", dbPath: "-", settingsPath: "-", eventCount: 0 }),
  settings_get: () => settings,
  settings_set: (a) => saveSettings(a.settings as Settings),
  sensors_pause: (a) => {
    const minutes = a.minutes as number | null;
    go("sleeping");
    return saveSettings({
      ...settings,
      pause:
        minutes == null
          ? { kind: "indefinite" }
          : { kind: "until", until: new Date(Date.now() + minutes * 60_000).toISOString() },
    });
  },
  sensors_resume: () => {
    go("idle");
    return saveSettings({ ...settings, pause: { kind: "none" } });
  },
  mascot_get: () => mascot,
  island_set_hit_rect: () => undefined,
  suggestion_current: () => suggestion,
  suggestion_choose: (a) => {
    const index = a.index as number;
    suggestion = null;
    emit("suggestion://clear", a.id);
    go("working");
    later(1400, () => {
      emit("action://result", {
        ok: index !== 2,
        message: index === 2 ? "Simulated failure" : "Opened in Zen",
        path: null,
        auto: false,
      });
      go(index === 2 ? "error" : "success");
    });
  },
  suggestion_dismiss: (a) => {
    suggestion = null;
    emit("suggestion://clear", a.id);
    go("idle");
  },
  events_recent: () => [],
  skills_list: () => [
    {
      id: "dev.open-in-browser",
      name: "Open dev servers in a browser",
      description: "Mock skill for the browser preview.",
      event: "port.listening",
      enabled: true,
      auto: false,
      autoByDefault: false,
      mutedUntil: null,
    },
    {
      id: "files.screenshot",
      name: "Screenshots",
      description: "Copy text out of a new screenshot.",
      event: "file.created",
      enabled: true,
      auto: true,
      autoByDefault: false,
      mutedUntil: null,
    },
    {
      id: "files.download",
      name: "Finished downloads",
      description: "Open or move a file when it lands.",
      event: "file.created",
      enabled: true,
      auto: false,
      autoByDefault: false,
      mutedUntil: new Date(Date.now() + 3 * 86_400_000).toISOString(),
    },
  ],
  skill_set: () => settings,
  capabilities_get: () => ({ found: ["Chrome (mock)"], skillsDir: "-", skillErrors: [] }),
  choices_reset: () => 0,
  actions_recent: () => [],
  reveal_path: () => undefined,
  open_settings: () => commands.ask_open?.({ view: "settings" }),
  debug_set_state: (a) => go(a.state as MascotState),
  debug_emit_event: () => {
    go("noticing");
    later(1200, () => go("idle"));
  },
  debug_demo_flow: () => {
    go("noticing");
    later(900, () => {
      suggestion = {
        id: crypto.randomUUID(),
        skillId: "debug.demo",
        title: "Dev server on localhost:3000",
        detail: "Demo suggestion (browser mock).",
        options: ["Open in Chrome", "Open in Zen", "Simulate failure"],
      };
      emit("suggestion://new", suggestion);
      go("suggesting");
    });
  },
};

// A fake streamed answer, so Ask mode can be developed in a browser.
function mockChat(a: Record<string, unknown>) {
  const id = a.id as string;
  const messages = a.messages as { content: string }[];
  const last = messages[messages.length - 1]?.content ?? "";
  const answer = `Found invoice-sept.pdf in Downloads, from Ali on Tuesday (preview, you asked "${last}").\nOPTION: Open invoice-sept.pdf\nOPTION: Show the folder\nOPTION: Find other invoices`;
  const words = answer.split(/(?<=\s)/);
  let i = 0;
  const tick = () => {
    if (i < words.length) {
      emit("ai://delta", { id, text: words[i++] });
      setTimeout(tick, 28);
    } else {
      const change = /\b(move|send|create|update|delete)\b/i.test(last);
      emit("ai://done", {
        id,
        provider: "local",
        error: null,
        handoff: change ? "needs a change (JIRA_TRANSITION_ISSUE)" : null,
      });
    }
  };
  emit("ai://tool", { id, name: "search" });
  later(1200, () => emit("ai://proposal", { chatId: id, id: `p-${id}`, label: "Move invoice-sept.pdf to Invoices" }));
  setTimeout(tick, 900);
}

commands.ai_chat = (a) => mockChat(a);
commands.ai_run_proposal = () => ({
  ok: true,
  message: "Moved invoice-sept.pdf to Invoices",
  undoId: 1,
  path: "C:/Users/you/Documents/Invoices/invoice-sept.pdf",
});
commands.ai_cancel = () => undefined;
let welcomeDeferred = false;
commands.ask_close = () => {
  if (!settings.onboarded && !welcomeDeferred) {
    commands.ask_open?.({ view: "welcome" });
    return;
  }
  emit("ask://close", { reason: "close" });
};
commands.ask_open = (a) => {
  welcomeDeferred = false;
  emit("ask://open", {
    context: {
      app: "Visual Studio Code",
      title: "Island.tsx - sidekick",
      clipboardKind: "stack_trace",
      clipboardPreview: "Error: listen EADDRINUSE: address already in use :::3000",
      clipboardSecret: false,
    },
    prompt: (a.prompt as string | null) ?? null,
    ask: Boolean(a.ask),
    clipboard: false,
    page: null,
    view: settings.onboarded ? ((a.view as string | undefined) ?? "ask") : "welcome",
    tool: (a.tool as string | undefined) ?? null,
  });
  if (!settings.onboarded && !welcomeSpoken) {
    welcomeSpoken = true;
    speakWelcome(settings.welcomeStep);
  }
};
let welcomeSpoken = false;
commands.ask_ensure_welcome = () => {
  if (settings.onboarded || welcomeDeferred) return;
  commands.ask_open?.({ view: "welcome" });
};
commands.ask_defer_welcome = () => {
  if (settings.onboarded) {
    emit("ask://close", { reason: "close" });
    return;
  }
  welcomeDeferred = true;
  emit("ask://close", { reason: "defer" });
};
commands.ask_resume_welcome = () => {
  if (settings.onboarded) return;
  welcomeDeferred = false;
  commands.ask_open?.({ view: "welcome" });
};
commands.skill_install = () => "Screenshots";
const voiceStatus = () => ({
  models: [
    { id: "wake", label: "Wake word", size: 17_626_723, installed: settings.voice.enabled },
    { id: "speech", label: "Speech to text", size: 57_267_600, installed: settings.voice.enabled },
    { id: "voice", label: "Supertonic voice", size: 128_774_318, installed: settings.voice.enabled },
  ],
  missingBytes: settings.voice.enabled ? 0 : 203_669_641,
  downloading: false,
  listening: settings.voice.enabled,
  error: null,
  voices: [
    { id: "f5", label: "Female 5 (warm)" },
    { id: "f1", label: "Female 1 (clear)" },
    { id: "m2", label: "Male 2 (deep, lively)" },
  ],
});
commands.voice_status = voiceStatus;
commands.clipboard_history = () => [
  { text: "npm run dev -- --port 3001", ts: new Date().toISOString() },
  { text: "https://github.com/tmalik258/sidekick/pull/5", ts: new Date().toISOString() },
  { text: "#0ea5e9", ts: new Date().toISOString() },
];
commands.clipboard_copy = () => undefined;
commands.search_clear = () => 0;
commands.setup_status = () => {
  const w = (id: string) => `winget install -e --id ${id} --accept-source-agreements`;
  const item = (
    id: string,
    group: "ai" | "connect" | "tools",
    title: string,
    why: string,
    done: boolean,
    status: string,
    extra: { command?: string; runnable?: boolean; tab?: string; recommended?: boolean } = {},
  ) => ({
    id,
    group,
    title,
    why,
    done,
    status,
    command: extra.command ?? null,
    runnable: extra.runnable ?? false,
    tab: extra.tab ?? null,
    recommended: extra.recommended ?? false,
  });
  return {
    items: [
      item("claude_code", "ai", "Claude Code", "Chat, drafts and skills with your Claude plan.", true, "Installed", {
        recommended: true,
      }),
      item("ollama", "ai", "Ollama", "Free local AI on this PC, and search by meaning.", true, "Running", {
        recommended: true,
      }),
      item("ollama_embed", "ai", "Search model", "Finds things by meaning. About 270 MB.", false, "Not downloaded", {
        command: "ollama pull nomic-embed-text",
        runnable: true,
        recommended: true,
      }),
      item("anthropic", "ai", "Anthropic API key", "Pay as you go instead of a Claude plan.", false, "Not set", {
        command: 'setx ANTHROPIC_API_KEY "your-key"',
      }),
      item(
        "claude_hooks",
        "connect",
        "Claude Code hooks",
        "Allow or deny requests from the island.",
        false,
        "Not added",
        {
          command: '{\n  "hooks": {}\n}',
          tab: "ai",
          recommended: true,
        },
      ),
      item("browser", "connect", "Browser extension", "Page summaries, form help and tabs.", false, "Not paired", {
        tab: "browser",
        recommended: true,
      }),
      item(
        "calendar",
        "connect",
        "Calendar",
        "Meeting reminders with Join and Prep.",
        Boolean(settings.composio.account),
        settings.composio.account ? "Connected" : "Not connected",
        { tab: "connections" },
      ),
      item("gh", "tools", "GitHub CLI", "Open PRs in the morning brief.", false, "Not installed", {
        command: w("GitHub.cli"),
        runnable: true,
        recommended: true,
      }),
      item("git", "tools", "Git", "Repo status and unsaved work.", true, "Installed", { recommended: true }),
      item("tesseract", "tools", "Tesseract", "Copy text out of screenshots.", false, "Not installed", {
        command: w("UB-Mannheim.TesseractOCR"),
        runnable: true,
        recommended: true,
      }),
      item("ffmpeg", "tools", "FFmpeg", "Convert videos and audio.", false, "Not installed", {
        command: w("Gyan.FFmpeg"),
        runnable: true,
      }),
    ],
    installAll: `${w("GitHub.cli")}; ${w("UB-Mannheim.TesseractOCR")}`,
  };
};
commands.setup_run = () => undefined;
commands.composio_test = () => ({
  tools: 42,
  reads: 30,
  sample: ["JIRA_SEARCH_ISSUES", "SLACK_LIST_CHANNELS", "GMAIL_FETCH_EMAILS"],
});
commands.composio_import = () => {
  settings = {
    ...settings,
    composio: { ...settings.composio, enabled: true, url: "https://mcp.composio.dev/example" },
  };
  emit("settings://changed", settings);
  return settings;
};
const COMPOSIO_APPS = [
  ["googlecalendar", "Google Calendar", "Meeting reminders and Join"],
  ["outlook", "Outlook", "Meetings and mail"],
  ["gmail", "Gmail", "Unread mail in the morning brief"],
  ["slack", "Slack", "Mentions in the morning brief"],
  ["jira", "Jira", "Your issues in the morning brief"],
  ["fathom", "Fathom", "Meeting notes and follow-ups"],
  ["github", "GitHub", "Reading issues and PRs"],
  ["notion", "Notion", "Reading your pages"],
];
const connected = new Set(["googlecalendar", "jira"]);
commands.composio_status = () => {
  const signedIn = Boolean(settings.composio.account);
  return {
    signedIn,
    account: settings.composio.account,
    apps: COMPOSIO_APPS.map(([slug, name, why]) => ({
      slug,
      name,
      why,
      logo: "",
      connected: signedIn && connected.has(slug),
    })),
    error: null,
  };
};
commands.composio_sign_in = () => {
  later(2500, () => {
    settings = {
      ...settings,
      composio: { ...settings.composio, enabled: true, account: "you@example.com", userId: "you@example.com" },
    };
    emit("settings://changed", settings);
    emit("composio://changed", { ok: true, message: "Signed in as you@example.com" });
  });
  return "K7Q2";
};
commands.composio_sign_out = () => {
  settings = { ...settings, composio: { ...settings.composio, enabled: false, account: "", userId: "" } };
  emit("settings://changed", settings);
  emit("composio://changed", { ok: true, message: "Signed out" });
};
commands.composio_connect = (a) => {
  later(2000, () => {
    connected.add(a.slug as string);
    emit("composio://changed", { ok: true, message: "Connected" });
  });
};
const folder = (path: string, label: string, repos = 0) => ({ path, label, repos });
commands.setup_detect = () => ({
  codeFolders: [
    folder("C:\\Users\\you\\code", "code", 12),
    folder("\\\\wsl.localhost\\Ubuntu-22.04\\home\\you\\projects", "projects (WSL Ubuntu-22.04)", 5),
  ],
  searchFolders: [
    folder("C:\\Users\\you\\Documents", "Documents"),
    folder("C:\\Users\\you\\Desktop", "Desktop"),
    folder("C:\\Users\\you\\Downloads", "Downloads"),
  ],
  chatModels: ["qwen3:1.7b"],
  embedModels: [],
  claudeInstalled: true,
  claudeHooks: false,
  claudeMcp: false,
  composioSignedIn: Boolean(settings.composio.account),
  composioInClaude: true,
  browsers: ["Chrome", "Edge", "Zen"],
  installable: [],
});
commands.setup_apply = () => {
  return ["Picked 2 code folders", "Added Claude Code hooks", "Installing the search model"];
};
commands.claude_add_hooks = () => "C:/Users/you/.claude/settings.json.sidekick-backup-20261002-101500";
commands.claude_add_mcp = () => undefined;
const browsersSeen = new Set<string>(["chrome"]);
commands.browsers_status = () => [
  { id: "chrome", name: "Chrome", connected: browsersSeen.has("chrome") },
  { id: "edge", name: "Edge", connected: browsersSeen.has("edge") },
  { id: "zen", name: "Zen", connected: browsersSeen.has("zen") },
];
commands.extension_install = (a) => {
  later(4000, () => browsersSeen.add(a.browser as string));
  return {
    copied: "C:\\Users\\you\\AppData\\Local\\Sidekick\\extension",
    page: "chrome://extensions/",
    steps: [
      "If the extensions page is not showing, paste its address into the address bar and press Enter (it is copied).",
      "Turn on Developer mode (top right), then click Load unpacked.",
      "Click Copy folder path below, paste it into the folder box and press Enter, then Select Folder.",
      "Press Allow on Sidekick's island when it asks.",
    ],
  };
};
commands.local_models = () => ({ reachable: true, chat: ["qwen3:1.7b", "llama3.2:3b"], embed: ["nomic-embed-text"] });
commands.running_apps = () => ["code.exe", "chrome.exe", "slack.exe", "windowsterminal.exe", "keepassxc.exe"];
commands.suggestion_always = (a) => commands.suggestion_choose?.(a);
let laterItems = [
  { id: "l1", title: "3 new screenshots", detail: "Copy text or move them to a folder", minutesAgo: 12, missed: true },
  { id: "l2", title: "Download finished", detail: "invoice-sept.pdf", minutesAgo: 25, missed: false },
];
commands.later_list = () => laterItems;
commands.later_open = (a) => {
  laterItems = laterItems.filter((l) => l.id !== a.id);
  emit("suggestion://later", laterItems.length);
};
commands.later_clear = () => {
  laterItems = [];
  emit("suggestion://later", 0);
};
commands.skill_unmute = () => undefined;
const chats = new Map<string, { title: string; updated: string; turns: unknown[] }>([
  [
    "c1",
    {
      title: "How do I free port 3000",
      updated: new Date(Date.now() - 3_600_000).toISOString(),
      turns: [
        { role: "user", content: "How do I free port 3000" },
        { role: "assistant", content: "Run `netstat -ano | findstr :3000`, then `taskkill /PID <pid> /F`." },
      ],
    },
  ],
]);
commands.chats_list = () =>
  [...chats.entries()]
    .map(([id, c]) => ({ id, title: c.title, updated: c.updated }))
    .sort((a, b) => b.updated.localeCompare(a.updated));
commands.chat_get = (a) => chats.get(a.id as string)?.turns ?? [];
commands.chat_save = (a) => {
  chats.set(a.id as string, {
    title: a.title as string,
    updated: new Date().toISOString(),
    turns: a.turns as unknown[],
  });
};
commands.chat_delete = (a) => {
  chats.delete(a.id as string);
};
commands.ai_handoff = () => "C:/Users/you/AppData/Local/Sidekick/ai/handoff";
commands.backup_export = () => "C:/Users/you/Documents/Sidekick backup.json";
commands.backup_import = () => "Restored settings and 2 skills";
commands.projects_list = () => [
  { name: "sidekick", path: "C:/Users/you/code/sidekick" },
  { name: "falconxoft-api", path: "C:/Users/you/code/falconxoft-api" },
];
commands.project_launch = () => "Opened sidekick in VS Code and a terminal";
commands.search_status = () => ({ items: 1240, embedded: 1240, embedError: null });
commands.calendar_today = () => ({
  meetings: connected.has("googlecalendar")
    ? [
        { title: "Standup", start: "10:00", end: "10:15", joinUrl: "https://meet.google.com/abc-defg-hij" },
        { title: "Design review", start: "15:00", end: "15:45", joinUrl: null },
      ]
    : [],
  error: null,
  sources: connected.has("googlecalendar") ? ["Google Calendar"] : [],
});
commands.voice_download = () => {
  let done = 0;
  const total = 203_669_641;
  const tick = () => {
    done = Math.min(total, done + 30_000_000);
    emit("voice://download", { label: "Supertonic voice", done, total, finished: done >= total, error: null });
    if (done < total) later(300, tick);
  };
  tick();
};
commands.voice_cancel_download = () => undefined;
commands.voice_listen = () => {
  const words = ["What", "What time", "What time is it", "What time is it in London?"];
  for (const [i, w] of words.entries()) {
    later(400 * (i + 1), () => emit("voice://heard", { text: w, final: false, byVoice: false }));
  }
  later(2200, () => emit("voice://heard", { text: words[3], final: true, byVoice: false }));
};
commands.voice_stop = () => {
  for (const t of speechTimers) clearTimeout(t);
  speechTimers = [];
};
// The welcome line as Rust would report it while it is spoken.
const WELCOME_LINES = [
  "Hey there! Welcome to the future! I'm Sidekick, your personal AI assistant. I had a quick look around, and here's what I found. Let's get you set up.",
  "First, my brain. I can think with Claude, or with a model that runs right here on your PC. Pick what you have, and I'll handle the rest.",
  "Now, your world. Connect your calendar, your mail and the tools you use, and I'll start noticing what matters.",
  "A few small helpers make me sharper. Install the ones you want, and I'll wait while they finish.",
  "Almost there. Talk to me anytime, just say Hey Sidekick. And I can start with Windows, so I'm here when you are.",
];
let welcome: WelcomeSpeech = {
  step: 0,
  lines: WELCOME_LINES,
  script: WELCOME_LINES[0],
  pieces: [],
  endsAt: null,
  silent: false,
  pending: false,
};
let speechTimers: ReturnType<typeof setTimeout>[] = [];
/** Like Rust: each step's line as sentences with when they sound. The first
 * one waits 3 s, like the model loading. */
function speakWelcome(step = 0) {
  for (const t of speechTimers) clearTimeout(t);
  speechTimers = [];
  const line = WELCOME_LINES[step] ?? WELCOME_LINES[0];
  const sentences = line.match(/[^.!?]+[.!?]/g) ?? [line];
  let at = Date.now() + (step === 0 ? 3000 : 400);
  welcome = { ...welcome, step, script: line, pieces: [], endsAt: null, pending: true };
  emit("voice://welcome", welcome);
  sentences.forEach((raw, i) => {
    const text = raw.trim();
    const ms = text.split(/\s+/).length * 330;
    const startsAt = at;
    at += ms + 280;
    // Pieces arrive a little ahead of when they sound, like synthesis.
    speechTimers.push(
      setTimeout(
        () => {
          welcome = { ...welcome, pieces: [...welcome.pieces, { text, startsAt, ms }] };
          if (i === sentences.length - 1) welcome = { ...welcome, endsAt: startsAt + ms, pending: false };
          emit("voice://welcome", welcome);
        },
        Math.max(0, startsAt - Date.now() - 300),
      ),
    );
  });
}
commands.voice_welcome = () => welcome;
commands.voice_welcome_step = (a) => speakWelcome(Number(a.step) || 0);
commands.voice_say = () => undefined;
commands.voice_test = () => undefined;
commands.search = (a) => [
  {
    source: "file",
    reference: "C:/Users/you/notes/acme.md",
    title: "acme.md",
    snippet: `Invoice for [${a.query}] Corp, due Friday ... rate 45 USD per hour`,
    ts: "2026-10-01T09:00:00Z",
  },
  {
    source: "chat",
    reference: "c1",
    title: "How do I free port 3000",
    snippet: `Use netstat -ano to find the [${a.query}] process`,
    ts: "2026-10-01T10:00:00Z",
  },
];
commands.search_reindex = () => undefined;
commands.open_reference = () => undefined;
commands.mcp_info = () => ({ url: "http://127.0.0.1:47823/mcp", token: "browser-preview-mcp-token" });
commands.time_today = () => [
  { app: "Visual Studio Code", project: "sidekick", secs: 9420 },
  { app: "Google Chrome", project: "", secs: 4310 },
  { app: "Windows Terminal", project: "", secs: 2200 },
  { app: "Visual Studio Code", project: "falconxoft-api", secs: 1500 },
  { app: "Slack", project: "", secs: 640 },
];
commands.browser_info = () => ({ token: "browser-preview-pairing-code", port: 47822 });
commands.action_undo = () => "Moved photo.webp to the Recycle Bin";
commands.ai_status = () => [
  { id: "claude_code", available: false, local: false },
  { id: "anthropic", available: false, local: false },
  { id: "local", available: true, local: true },
  { id: "semif", available: false, local: true },
];

export const mock = {
  async invoke<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
    const fn = commands[cmd];
    if (!fn) throw new Error(`mock: unknown command ${cmd}`);
    return fn(args) as T;
  },
  listen(event: string, handler: Handler): () => void {
    let set = handlers.get(event);
    if (!set) {
      set = new Set();
      handlers.set(event, set);
    }
    set.add(handler);
    return () => set.delete(handler);
  },
};

// Exposed for console debugging in the browser preview.
if (typeof window !== "undefined") {
  (window as unknown as { sidekickMock: unknown }).sidekickMock = { go, cues: CUES, invoke: mock.invoke, emit };
}
