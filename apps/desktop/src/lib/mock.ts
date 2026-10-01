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
    later(1400, () => go(index === 2 ? "error" : "success"));
  },
  suggestion_dismiss: (a) => {
    suggestion = null;
    emit("suggestion://clear", a.id);
    go("idle");
  },
  events_recent: () => [],
  open_settings: () => {
    window.open("/settings/", "_blank");
  },
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
        title: "Dev server on localhost:3000",
        detail: "Demo suggestion (browser mock).",
        options: ["Open in Chrome", "Open in Zen", "Simulate failure"],
      };
      emit("suggestion://new", suggestion);
      go("suggesting");
    });
  },
};

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
  (window as unknown as { sidekickMock: unknown }).sidekickMock = { go, cues: CUES, invoke: mock.invoke };
}
