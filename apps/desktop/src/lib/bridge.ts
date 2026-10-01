// Talks to the Rust core through Tauri. Outside Tauri (plain `pnpm web` in a
// browser) it falls back to an in-memory mock so the UI can be developed and
// previewed without the desktop shell.

import { mock } from "./mock";
import type { AppInfo, HitRect, MascotState, Settings, StoredEvent, Suggestion, Transition } from "./types";

export const EVENTS = {
  mascotState: "mascot://state",
  suggestionNew: "suggestion://new",
  suggestionClear: "suggestion://clear",
  islandHover: "island://hover",
  settingsChanged: "settings://changed",
} as const;

export interface EventPayloads {
  [EVENTS.mascotState]: Transition;
  [EVENTS.suggestionNew]: Suggestion;
  [EVENTS.suggestionClear]: string;
  [EVENTS.islandHover]: boolean;
  [EVENTS.settingsChanged]: Settings;
}

export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) return mock.invoke<T>(cmd, args);
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args);
}

export async function listen<K extends keyof EventPayloads>(
  event: K,
  handler: (payload: EventPayloads[K]) => void,
): Promise<() => void> {
  if (!isTauri()) return mock.listen(event, handler as (payload: unknown) => void);
  const { listen } = await import("@tauri-apps/api/event");
  return listen<EventPayloads[K]>(event, (e) => handler(e.payload));
}

export const api = {
  appInfo: () => invoke<AppInfo>("app_info"),
  settingsGet: () => invoke<Settings>("settings_get"),
  settingsSet: (settings: Settings) => invoke<Settings>("settings_set", { settings }),
  sensorsPause: (minutes: number | null) => invoke<Settings>("sensors_pause", { minutes }),
  sensorsResume: () => invoke<Settings>("sensors_resume"),
  mascotGet: () => invoke<MascotState>("mascot_get"),
  islandSetHitRect: (rect: HitRect) => invoke<void>("island_set_hit_rect", { rect }),
  suggestionCurrent: () => invoke<Suggestion | null>("suggestion_current"),
  suggestionChoose: (id: string, index: number) => invoke<void>("suggestion_choose", { id, index }),
  suggestionDismiss: (id: string, reason: "user" | "timeout") => invoke<void>("suggestion_dismiss", { id, reason }),
  eventsRecent: (limit = 50) => invoke<StoredEvent[]>("events_recent", { limit }),
  openSettings: () => invoke<void>("open_settings"),
  debugSetState: (state: MascotState) => invoke<void>("debug_set_state", { state }),
  debugEmitEvent: () => invoke<void>("debug_emit_event"),
  debugDemoFlow: () => invoke<void>("debug_demo_flow"),
};
