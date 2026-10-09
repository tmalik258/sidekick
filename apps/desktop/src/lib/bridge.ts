// Talks to the Rust core through Tauri. Outside Tauri (plain `pnpm web` in a
// browser) it falls back to an in-memory mock so the UI can be developed and
// previewed without the desktop shell.

import { mock } from "./mock";
import type {
  ActionRecord,
  ActionResult,
  AgentMode,
  AgentStarted,
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
  CloudId,
  ComposioStatus,
  CursorChat,
  EditorList,
  ExtensionGuide,
  FileChange,
  Found,
  HitRect,
  InboxStatus,
  InstantResults,
  LaterItem,
  Learned,
  LocalModels,
  MascotState,
  MergeState,
  NotifyLevel,
  ProviderStatus,
  Recipe,
  ReposOverview,
  RoutineItem,
  SearchHit,
  Settings,
  SetupPlan,
  SetupStatus,
  SkillInfo,
  StoredEvent,
  Suggestion,
  SuggestionRate,
  Timing,
  Transition,
  Turn,
  UpdateInfo,
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
  voiceSpeaking: "voice://speaking",
  agentEvent: "agent://event",
  voiceDownload: "voice://download",
  suggestionLater: "suggestion://later",
  composioChanged: "composio://changed",
  browsersChanged: "browsers://changed",
  guideKey: "guide://key",
  voiceWelcome: "voice://welcome",
  netStatus: "net://status",
  updateAvailable: "update://available",
  timing: "timing://recorded",
  islandDone: "island://done",
  islandFocus: "island://focus",
} as const;

export interface EventPayloads {
  [EVENTS.mascotState]: Transition;
  [EVENTS.islandDone]: { id: string; text: string };
  [EVENTS.islandFocus]: { until: number | null; held: number | unknown[]; ended?: boolean };
  [EVENTS.suggestionNew]: Suggestion;
  [EVENTS.suggestionClear]: string;
  [EVENTS.islandHover]: boolean;
  [EVENTS.islandCursor]: { x: number; y: number };
  [EVENTS.islandVisible]: boolean;
  [EVENTS.islandFullscreen]: boolean;
  [EVENTS.netStatus]: boolean;
  [EVENTS.updateAvailable]: UpdateInfo;
  [EVENTS.inboxChanged]: null;
  [EVENTS.aiDelta]: { id: string; text: string };
  [EVENTS.aiDone]: {
    id: string;
    provider: string | null;
    error: string | null;
    handoff: string | null;
    /** US dollars, only for Anthropic API answers. */
    cost?: number | null;
  };
  [EVENTS.aiTool]: { id: string; name: string; label?: string };
  [EVENTS.aiProposal]: { chatId: string; id: string; label: string; step?: boolean };
  [EVENTS.askOpen]: AskOpen;
  [EVENTS.askClose]: { reason: "close" | "defer" };
  [EVENTS.actionResult]: ActionResult;
  [EVENTS.settingsChanged]: Settings;
  [EVENTS.voiceState]: VoiceStatus;
  [EVENTS.voiceHeard]: VoiceHeard;
  [EVENTS.voiceSpeaking]: boolean;
  [EVENTS.agentEvent]: { session: string; kind: string } & Record<string, unknown>;
  [EVENTS.voiceDownload]: VoiceDownload;
  [EVENTS.suggestionLater]: number;
  [EVENTS.composioChanged]: { ok: boolean; message: string };
  [EVENTS.browsersChanged]: null;
  [EVENTS.guideKey]: number;
  [EVENTS.voiceWelcome]: WelcomeSpeech;
  [EVENTS.timing]: Timing;
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

/** A model OpenRouter offers; prices in US dollars per million tokens. */
export interface RouterModel {
  id: string;
  name: string;
  free: boolean;
  input: number;
  output: number;
  context: number;
  tools: boolean;
}

export const api = {
  appInfo: () => invoke<AppInfo>("app_info"),
  systemLook: () => invoke<{ transparency: boolean; batterySaver: boolean }>("system_look"),
  diagnostics: () => invoke<string>("diagnostics"),
  reportSave: () => invoke<string>("report_save"),
  cloudKeys: () => invoke<Record<CloudId, boolean>>("cloud_keys"),
  cloudKeySet: (id: CloudId, key: string) => invoke<Settings>("cloud_key_set", { id, key }),
  cloudKeyClear: (id: CloudId) => invoke<Settings>("cloud_key_clear", { id }),
  openrouterModels: () => invoke<RouterModel[]>("openrouter_models"),
  crashPending: () => invoke<string | null>("crash_pending"),
  crashDismiss: () => invoke<void>("crash_dismiss"),
  settingsGet: () => invoke<Settings>("settings_get"),
  settingsSet: (settings: Settings) => invoke<Settings>("settings_set", { settings }),
  sensorsPause: (minutes: number | null) => invoke<Settings>("sensors_pause", { minutes }),
  sensorsResume: () => invoke<Settings>("sensors_resume"),
  mascotGet: () => invoke<MascotState>("mascot_get"),
  islandSetHitRect: (rect: HitRect) => invoke<void>("island_set_hit_rect", { rect }),
  islandReady: () => invoke<void>("island_ready"),
  netStatus: () => invoke<boolean>("net_status"),
  updateStatus: () => invoke<UpdateInfo | null>("update_status"),
  updateCheck: () => invoke<UpdateInfo | null>("update_check"),
  updateInstall: () => invoke<string>("update_install"),
  netCheck: (lost: boolean) => invoke<boolean>("net_check", { lost }),
  suggestionCurrent: () => invoke<Suggestion | null>("suggestion_current"),
  suggestionChoose: (id: string, index: number, priv = false) =>
    invoke<void>("suggestion_choose", { id, index, private: priv }),
  suggestionDismiss: (id: string, reason: "user" | "timeout") => invoke<void>("suggestion_dismiss", { id, reason }),
  eventsRecent: (limit = 50) => invoke<StoredEvent[]>("events_recent", { limit }),
  openSettings: () => invoke<void>("open_settings"),
  debugSetState: (state: MascotState) => invoke<void>("debug_set_state", { state }),
  debugEmitEvent: () => invoke<void>("debug_emit_event"),
  skillsList: () => invoke<SkillInfo[]>("skills_list"),
  skillSet: (id: string, enabled: boolean, auto: boolean) => invoke<Settings>("skill_set", { id, enabled, auto }),
  capabilitiesGet: (rescan = false) => invoke<CapabilityInfo>("capabilities_get", { rescan }),
  choicesReset: () => invoke<number>("choices_reset"),
  routinesToday: () => invoke<RoutineItem[]>("routines_today"),
  routinesForget: () => invoke<number>("routines_forget"),
  learnedList: () => invoke<Learned[]>("learned_list"),
  suggestionRates: () => invoke<SuggestionRate[]>("suggestion_rates"),
  diskGroups: () => invoke<DiskGroup[]>("disk_groups"),
  diskClean: (group: string, paths: string[]) => invoke<string>("disk_clean", { group, paths }),
  userGuessName: () => invoke<string>("user_guess_name"),
  focusStart: (minutes?: number) => invoke<string>("focus_start", { minutes }),
  focusStop: () => invoke<string>("focus_stop"),
  focusStatus: () => invoke<{ until: number | null; held: number }>("focus_status"),
  learnedForget: (kind: string, key: string, label: string) => invoke<void>("learned_forget", { kind, key, label }),
  learnedForgetAll: () => invoke<void>("learned_forget_all"),
  routinesRemove: (kind: string, key: string) => invoke<number>("routines_remove", { kind, key }),
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
      /** Asked by voice: answer in short spoken sentences. */
      voice?: boolean;
      /** Provider picked in Ask mode; null lets Sidekick choose. */
      prefer?: string | null;
      /** Started early at a pause in speech: hidden until aiRelease. */
      hold?: boolean;
      /** "Think harder": let the local model reason before answering. */
      think?: boolean;
    },
    localOnly: boolean,
  ) => invoke<void>("ai_chat", { id, messages, attach, localOnly }),
  aiCancel: (id: string) => invoke<void>("ai_cancel", { id }),
  aiRelease: (id: string) => invoke<void>("ai_release", { id }),
  timingRecord: (name: Timing["name"], ms: number) => invoke<void>("timing_record", { name, ms }),
  timingsRecent: () => invoke<Timing[]>("timings_recent"),
  askClose: () => invoke<void>("ask_close"),
  browserInfo: () => invoke<BrowserInfo>("browser_info"),
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
  editorsList: () => invoke<EditorList>("editors_list"),
  instantFind: (query: string) => invoke<InstantResults>("instant_find", { query }),
  appLaunch: (id: string, opts?: { private?: boolean; browser?: string }) =>
    invoke<void>("app_launch", {
      id,
      private: opts?.private ?? null,
      browser: opts?.browser ?? null,
    }),
  fileOpen: (path: string, how?: "editor" | "reveal" | "default") =>
    invoke<{ opened: boolean }>("file_open", { path, how: how ?? null }),
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
  agentUsual: (path: string) => invoke<string | null>("agent_usual", { path }),
  agentStart: (
    agent: string,
    path: string,
    prompt: string,
    mode: AgentMode,
    model: string | null = null,
    effort: string | null = null,
  ) => invoke<AgentStarted>("agent_start", { agent, path, prompt, mode, model, effort }),
  agentHandoff: (messages: ChatMessage[], reason: string | null) =>
    invoke<AgentStarted>("agent_handoff", { messages, reason }),
  agentSend: (id: string, text: string) => invoke<void>("agent_send", { id, text }),
  agentTune: (id: string, model: string | null, effort: string | null) =>
    invoke<void>("agent_tune", { id, model, effort }),
  agentFinish: (id: string) => invoke<string>("agent_finish", { id }),
  agentStop: (id: string) => invoke<void>("agent_stop", { id }),
  agentAnswer: (question: string, answer: "allow" | "always" | "deny") =>
    invoke<void>("agent_answer", { question, answer }),
  agentChanges: (id: string) => invoke<FileChange[]>("agent_changes", { id }),
  agentUndo: (id: string, path: string | null, hunk: number | null) => invoke<void>("agent_undo", { id, path, hunk }),
  agentClose: (id: string) => invoke<void>("agent_close", { id }),
  agentTerminal: (id: string) => invoke<void>("agent_terminal", { id }),
  agentResume: (id: string) => invoke<void>("agent_resume", { id }),
  windowsSettingsOpen: (page: string) => invoke<void>("windows_settings_open", { page }),
  pcSwitch: (name: string, on: boolean) => invoke<string>("pc_switch", { name, on }),
  agentMemory: (id: string) => invoke<number | null>("agent_memory", { id }),
  agentOpenEditor: (id: string) => invoke<void>("agent_open_editor", { id }),
  cursorChats: () => invoke<CursorChat[]>("cursor_chats"),
  cursorOpen: (path: string) => invoke<void>("cursor_open", { path }),
  agentRewindPreview: (id: string, index: number) => invoke<number>("agent_rewind_preview", { id, index }),
  agentRewind: (id: string, index: number) => invoke<number>("agent_rewind", { id, index }),
  agentFiles: (id: string, query: string) => invoke<string[]>("agent_files", { id, query }),
  agentCommands: (id: string) =>
    invoke<{ name: string; description: string; group: string }[]>("agent_commands", { id }),
  aiOpenLink: (target: string) => invoke<string>("ai_open_link", { target }),
  reposOverview: (fresh: boolean) => invoke<ReposOverview>("repos_overview", { fresh }),
  repoPull: (path: string, stash: boolean) => invoke<string>("repo_pull", { path, stash }),
  repoOpen: (path: string) => invoke<void>("repo_open", { path }),
  mergeState: (path: string) => invoke<MergeState>("merge_state", { path }),
  mergeKeep: (path: string, file: string, side: "mine" | "theirs" | "both") =>
    invoke<void>("merge_keep", { path, file, side }),
  mergeEnd: (path: string, finish: boolean) => invoke<ActionResult>("merge_end", { path, finish }),
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
  voiceRead: (text: string) => invoke<void>("voice_read", { text }),
  askOpen: (prompt: string | null = null, ask = false) => invoke<void>("ask_open", { prompt, ask }),
  askEnsureWelcome: () => invoke<void>("ask_ensure_welcome"),
  askDeferWelcome: () => invoke<void>("ask_defer_welcome"),
  askResumeWelcome: () => invoke<void>("ask_resume_welcome"),
};

/** One group in the Storage view. */
export interface DiskGroup {
  id: string;
  label: string;
  what: string;
  bytes: number;
  partial: boolean;
  items: { path: string; bytes: number }[];
  clearable: boolean;
}
