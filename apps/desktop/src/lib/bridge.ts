// Talks to the Rust core through Tauri. Outside Tauri (plain `pnpm web` in a
// browser) it falls back to an in-memory mock so the UI can be developed and
// previewed without the desktop shell.

import { mock } from "./mock";
import type {
  ActionRecord,
  ActionResult,
  Agents,
  AppInfo,
  AppTime,
  AskOpen,
  BrowserInfo,
  BrowserStatus,
  CalendarToday,
  CapabilityInfo,
  ChatMessage,
  ChatSummary,
  ComposioStatus,
  ExtensionGuide,
  Found,
  HitRect,
  InboxStatus,
  LaterItem,
  LocalModels,
  MascotState,
  NotifyLevel,
  PasswordBrowserInfo,
  PasswordEditDraft,
  PasswordMirrorStatus,
  PasswordPrompt,
  PasswordSaved,
  ProviderStatus,
  Recipe,
  RoutineItem,
  SearchHit,
  Settings,
  SetupPlan,
  SetupStatus,
  SkillInfo,
  StoredEvent,
  Suggestion,
  Transition,
  Turn,
  VoiceDownload,
  VoiceHeard,
  VoiceStatus,
  WelcomeSpeech,
} from "./types";

export const EVENTS = {
  mascotState: "mascot://state",
  suggestionNew: "suggestion://new",
  suggestionClear: "suggestion://clear",
  islandHover: "island://hover",
  islandCursor: "island://cursor",
  islandVisible: "island://visible",
  islandFullscreen: "island://fullscreen",
  inboxChanged: "inbox://changed",
  aiDelta: "ai://delta",
  aiDone: "ai://done",
  aiTool: "ai://tool",
  aiProposal: "ai://proposal",
  askOpen: "ask://open",
  askClose: "ask://close",
  actionResult: "action://result",
  settingsChanged: "settings://changed",
  voiceState: "voice://state",
  voiceHeard: "voice://heard",
  voiceDownload: "voice://download",
  suggestionLater: "suggestion://later",
  composioChanged: "composio://changed",
  guideKey: "guide://key",
  voiceWelcome: "voice://welcome",
  passwordSaved: "password://saved",
  passwordMirror: "password://mirror",
  netStatus: "net://status",
} as const;

export interface EventPayloads {
  [EVENTS.mascotState]: Transition;
  [EVENTS.suggestionNew]: Suggestion;
  [EVENTS.suggestionClear]: string;
  [EVENTS.islandHover]: boolean;
  [EVENTS.islandCursor]: { x: number; y: number };
  [EVENTS.islandVisible]: boolean;
  [EVENTS.islandFullscreen]: boolean;
  [EVENTS.netStatus]: boolean;
  [EVENTS.inboxChanged]: null;
  [EVENTS.aiDelta]: { id: string; text: string };
  [EVENTS.aiDone]: { id: string; provider: string | null; error: string | null; handoff: string | null };
  [EVENTS.aiTool]: { id: string; name: string; label?: string };
  [EVENTS.aiProposal]: { chatId: string; id: string; label: string };
  [EVENTS.askOpen]: AskOpen;
  [EVENTS.askClose]: { reason: "close" | "defer" };
  [EVENTS.actionResult]: ActionResult;
  [EVENTS.settingsChanged]: Settings;
  [EVENTS.voiceState]: VoiceStatus;
  [EVENTS.voiceHeard]: VoiceHeard;
  [EVENTS.voiceDownload]: VoiceDownload;
  [EVENTS.suggestionLater]: number;
  [EVENTS.passwordSaved]: PasswordSaved;
  [EVENTS.passwordMirror]: PasswordMirrorStatus;
  [EVENTS.composioChanged]: { ok: boolean; message: string };
  [EVENTS.guideKey]: number;
  [EVENTS.voiceWelcome]: WelcomeSpeech;
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
  islandReady: () => invoke<void>("island_ready"),
  netStatus: () => invoke<boolean>("net_status"),
  netCheck: (lost: boolean) => invoke<boolean>("net_check", { lost }),
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
  routinesToday: () => invoke<RoutineItem[]>("routines_today"),
  routinesForget: () => invoke<number>("routines_forget"),
  actionsRecent: (limit = 30) => invoke<ActionRecord[]>("actions_recent", { limit }),
  revealPath: (path: string) => invoke<void>("reveal_path", { path }),
  aiStatus: () => invoke<ProviderStatus[]>("ai_status"),
  aiChat: (
    id: string,
    messages: ChatMessage[],
    attach: {
      window: boolean;
      clipboard: boolean;
      page?: string | null;
      skill?: boolean;
      screen?: boolean;
      speak?: boolean;
      /** Provider picked in Ask mode; null lets Sidekick choose. */
      prefer?: string | null;
    },
    localOnly: boolean,
  ) => invoke<void>("ai_chat", { id, messages, attach, localOnly }),
  aiCancel: (id: string) => invoke<void>("ai_cancel", { id }),
  askClose: () => invoke<void>("ask_close"),
  browserInfo: () => invoke<BrowserInfo>("browser_info"),
  passwordSaveStatus: (id: string, displayed = false) =>
    invoke<PasswordPrompt>("password_save_status", { id, displayed }),
  passwordSaveDraft: (id: string) => invoke<PasswordEditDraft>("password_save_draft", { id }),
  passwordSaveCommit: (id: string, overrideExisting: boolean, username?: string, password?: string, source?: string) =>
    invoke<string>("password_save_commit", {
      id,
      overrideExisting,
      username: username ?? null,
      password: password ?? null,
      source: source ?? null,
    }),
  passwordSaveCancel: (id: string) => invoke<void>("password_save_cancel", { id }),
  passwordsMirror: () => invoke<string>("passwords_mirror"),
  passwordsMirrorStatus: () => invoke<PasswordMirrorStatus>("passwords_mirror_status"),
  passwordsMirrorCancel: () => invoke<void>("passwords_mirror_cancel"),
  passwordBrowsers: () => invoke<PasswordBrowserInfo[]>("password_browsers"),
  timeToday: () => invoke<AppTime[]>("time_today"),
  skillInstall: (yaml: string) => invoke<string>("skill_install", { yaml }),
  search: (query: string) => invoke<SearchHit[]>("search", { query }),
  searchStatus: () => invoke<{ items: number; embedded: number; embedError: string | null }>("search_status"),
  searchReindex: () => invoke<void>("search_reindex"),
  openReference: (source: string, reference: string) => invoke<void>("open_reference", { source, reference }),
  actionUndo: (id: number) => invoke<string>("action_undo", { id }),
  clipboardHistory: (limit = 60) => invoke<{ text: string; ts: string }[]>("clipboard_history", { limit }),
  clipboardCopy: (text: string) => invoke<void>("clipboard_copy", { text }),
  projectsList: () => invoke<{ name: string; path: string }[]>("projects_list"),
  projectLaunch: (path: string) => invoke<string>("project_launch", { path }),
  aiHandoff: (messages: ChatMessage[], reason: string | null) => invoke<string>("ai_handoff", { messages, reason }),
  aiRunProposal: (id: string) =>
    invoke<{ ok: boolean; message: string; undoId: number | null; path: string | null }>("ai_run_proposal", { id }),
  composioImport: () => invoke<Settings>("composio_import"),
  composioSignIn: () => invoke<string>("composio_sign_in"),
  composioSignOut: () => invoke<void>("composio_sign_out"),
  composioStatus: () => invoke<ComposioStatus>("composio_status"),
  composioConnect: (slug: string) => invoke<void>("composio_connect", { slug }),
  composioUseKey: (key: string) => invoke<string>("composio_use_key", { key }),
  agentsStatus: () => invoke<Agents>("agents_status"),
  aiOpenLink: (target: string) => invoke<string>("ai_open_link", { target }),
  codexAddNotify: () => invoke<string | null>("codex_add_notify"),
  codexAddMcp: () => invoke<string | null>("codex_add_mcp"),
  guideKeys: (buttons: number) => invoke<void>("guide_keys", { buttons }),
  notificationsStatus: () => invoke<InboxStatus>("notifications_status"),
  notificationsSetLevel: (from: string, level: NotifyLevel | "auto") =>
    invoke<string>("notifications_set_level", { from, level }),
  dndGet: () => invoke<boolean | null>("dnd_get"),
  dndSet: (on: boolean) => invoke<string>("dnd_set", { on }),
  recipeSave: (recipe: Recipe) => invoke<string>("recipe_save", { recipe }),
  recipeDelete: (id: string) => invoke<string>("recipe_delete", { id }),
  recipeRun: (id: string) => invoke<string>("recipe_run", { id }),
  knowHowClear: () => invoke<void>("know_how_clear"),
  setupDetect: () => invoke<Found>("setup_detect"),
  setupApply: (plan: SetupPlan) => invoke<string[]>("setup_apply", { plan }),
  claudeAddHooks: () => invoke<string | null>("claude_add_hooks"),
  claudeAddMcp: () => invoke<void>("claude_add_mcp"),
  browsersStatus: () => invoke<BrowserStatus[]>("browsers_status"),
  extensionInstall: (browser: string) => invoke<ExtensionGuide>("extension_install", { browser }),
  localModels: () => invoke<LocalModels>("local_models"),
  runningApps: () => invoke<string[]>("running_apps"),
  suggestionAlways: (id: string, index: number) => invoke<void>("suggestion_always", { id, index }),
  laterList: () => invoke<LaterItem[]>("later_list"),
  laterOpen: (id: string) => invoke<void>("later_open", { id }),
  laterClear: () => invoke<void>("later_clear"),
  skillUnmute: (id: string) => invoke<void>("skill_unmute", { id }),
  chatsList: () => invoke<ChatSummary[]>("chats_list"),
  chatGet: (id: string) => invoke<Turn[]>("chat_get", { id }),
  chatSave: (id: string, title: string, turns: Turn[]) => invoke<void>("chat_save", { id, title, turns }),
  chatDelete: (id: string) => invoke<void>("chat_delete", { id }),
  setupStatus: () => invoke<SetupStatus>("setup_status"),
  setupRun: (id: string) => invoke<void>("setup_run", { id }),
  searchClear: () => invoke<number>("search_clear"),
  backupExport: () => invoke<string>("backup_export"),
  backupImport: (text: string) => invoke<string>("backup_import", { text }),
  calendarToday: () => invoke<CalendarToday>("calendar_today"),
  voiceStatus: () => invoke<VoiceStatus>("voice_status"),
  voiceDownload: () => invoke<void>("voice_download"),
  voiceCancelDownload: () => invoke<void>("voice_cancel_download"),
  voiceListen: () => invoke<void>("voice_listen"),
  voiceStop: () => invoke<void>("voice_stop"),
  voiceTest: () => invoke<void>("voice_test"),
  voiceWelcome: () => invoke<WelcomeSpeech>("voice_welcome"),
  voiceWelcomeStep: (step: number) => invoke<void>("voice_welcome_step", { step }),
  voiceSay: (text: string) => invoke<void>("voice_say", { text }),
  askOpen: (prompt: string | null = null, ask = false) => invoke<void>("ask_open", { prompt, ask }),
  askEnsureWelcome: () => invoke<void>("ask_ensure_welcome"),
  askDeferWelcome: () => invoke<void>("ask_defer_welcome"),
  askResumeWelcome: () => invoke<void>("ask_resume_welcome"),
};
