// Browser-only stand-in for the Rust core, used when the UI runs outside
// Tauri. It mimics the demo flow loosely; the real rules live in Rust.

import { CUES, type Cue, DEFAULT_SETTINGS, type MascotState, type Settings, type Suggestion } from "./types";

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
  const answer = `This is the browser preview, so no AI is connected.\n\nYou asked: "${last}"\n\n\`\`\`powershell\nwinget install Ollama.Ollama\nollama pull qwen3:4b\n\`\`\``;
  const words = answer.split(/(?<=\s)/);
  let i = 0;
  const tick = () => {
    if (i < words.length) {
      emit("ai://delta", { id, text: words[i++] });
      setTimeout(tick, 28);
    } else {
      emit("ai://done", { id, provider: "local", error: null });
    }
  };
  setTimeout(tick, 300);
}

commands.ai_chat = (a) => mockChat(a);
commands.ai_cancel = () => undefined;
commands.ask_close = () => emit("ask://close", null);
commands.ask_open = (a) =>
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
    view: (a.view as string | undefined) ?? "ask",
  });
commands.skill_install = () => "Screenshots";
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
commands.search_status = () => ({ items: 1284 });
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
