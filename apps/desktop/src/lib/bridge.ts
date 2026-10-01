// Talks to the Rust core through Tauri. Outside Tauri (plain `pnpm web` in a
// browser) it falls back to an in-memory mock so the UI can be developed and
// previewed without the desktop shell.

import { mock } from "./mock";
import type {
  ActionRecord,
  ActionResult,
  AppInfo,
  AppTime,
  AskOpen,
  BrowserInfo,
  CapabilityInfo,
  ChatMessage,
  HitRect,
  MascotState,
  McpInfo,
  ProviderStatus,
  SearchHit,
  Settings,
  SkillInfo,
  StoredEvent,
  Suggestion,
  Transition,
} from "./types";

export const EVENTS = {
  mascotState: "mascot://state",
  suggestionNew: "suggestion://new",
  suggestionClear: "suggestion://clear",
  islandHover: "island://hover",
  islandCursor: "island://cursor",
  islandVisible: "island://visible",
  aiDelta: "ai://delta",
  aiDone: "ai://done",
  askOpen: "ask://open",
  askClose: "ask://close",
  actionResult: "action://result",
  settingsChanged: "settings://changed",
} as const;

export interface EventPayloads {
  [EVENTS.mascotState]: Transition;
  [EVENTS.suggestionNew]: Suggestion;
  [EVENTS.suggestionClear]: string;
  [EVENTS.islandHover]: boolean;
  [EVENTS.islandCursor]: { x: number; y: number };
  [EVENTS.islandVisible]: boolean;
  [EVENTS.aiDelta]: { id: string; text: string };
  [EVENTS.aiDone]: { id: string; provider: string | null; error: string | null };
  [EVENTS.askOpen]: AskOpen;
  [EVENTS.askClose]: null;
  [EVENTS.actionResult]: ActionResult;
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
  skillsList: () => invoke<SkillInfo[]>("skills_list"),
  skillSet: (id: string, enabled: boolean, auto: boolean) => invoke<Settings>("skill_set", { id, enabled, auto }),
  capabilitiesGet: (rescan = false) => invoke<CapabilityInfo>("capabilities_get", { rescan }),
  choicesReset: () => invoke<number>("choices_reset"),
  actionsRecent: (limit = 30) => invoke<ActionRecord[]>("actions_recent", { limit }),
  revealPath: (path: string) => invoke<void>("reveal_path", { path }),
  aiStatus: () => invoke<ProviderStatus[]>("ai_status"),
  aiChat: (
    id: string,
    messages: ChatMessage[],
    attach: { window: boolean; clipboard: boolean; page?: string | null; skill?: boolean; screen?: boolean },
    localOnly: boolean,
  ) => invoke<void>("ai_chat", { id, messages, attach, localOnly }),
  aiCancel: (id: string) => invoke<void>("ai_cancel", { id }),
  askClose: () => invoke<void>("ask_close"),
  browserInfo: () => invoke<BrowserInfo>("browser_info"),
  timeToday: () => invoke<AppTime[]>("time_today"),
  skillInstall: (yaml: string) => invoke<string>("skill_install", { yaml }),
  search: (query: string) => invoke<SearchHit[]>("search", { query }),
  searchStatus: () => invoke<{ items: number }>("search_status"),
  searchReindex: () => invoke<void>("search_reindex"),
  openReference: (source: string, reference: string) => invoke<void>("open_reference", { source, reference }),
  mcpInfo: () => invoke<McpInfo>("mcp_info"),
  actionUndo: (id: number) => invoke<string>("action_undo", { id }),
  askOpen: (prompt: string | null = null, ask = false) => invoke<void>("ask_open", { prompt, ask }),
};
