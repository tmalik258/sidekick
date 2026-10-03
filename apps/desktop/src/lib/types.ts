// Mirrors the Rust types in crates/core and src-tauri. Keep in sync.

export const MASCOT_STATES = [
  "idle",
  "sleeping",
  "noticing",
  "suggesting",
  "listening",
  "working",
  "success",
  "error",
] as const;

export type MascotState = (typeof MASCOT_STATES)[number];

export const CUES = ["yawn", "settle", "chirp", "pop", "open", "ding", "boop"] as const;

export type Cue = (typeof CUES)[number];

export interface Transition {
  previous: MascotState;
  state: MascotState;
  cue: Cue | null;
}

export type Pause = { kind: "none" } | { kind: "until"; until: string } | { kind: "indefinite" };

export interface Settings {
  muted: boolean;
  masterVolume: number;
  cueVolumes: Record<string, number>;
  collapseAfterSecs: number;
  launchAtLogin: boolean;
  sensors: Record<string, boolean>;
  pause: Pause;
  theme: Theme;
  soundKit: string;
  skills?: Record<string, { enabled?: boolean | null; auto?: boolean | null }>;
  paletteHotkey: string;
  /** Global shortcuts by action; empty turns one off. */
  shortcuts: Record<string, string>;
  codeFolders: string[];
  indexFolders: string[];
  endOfDayHour: number;
  ai: AiSettings;
  voice: VoiceSettings;
  calendar: { remindMinutes: number };
  semanticSearch: { enabled: boolean; model: string };
  onboarded: boolean;
  /** 0-based welcome step; resumed until onboarded is true. */
  welcomeStep: number;
  checkUpdates: boolean;
  composio: ComposioSettings;
  denyApps: string[];
  denySites: string[];
  routines: boolean;
  routinesAuto: boolean;
  /** Fade the island while a fullscreen app is in front. */
  hideInFullscreen: boolean;
  notifications: NotificationSettings;
  recipes: Recipe[];
  memory: string[];
  agent: AgentSettings;
  /** Browser ids for password fill/save. Null means all detected stores; [] disables all. */
  passwordBrowsers: string[] | null;
  passwordSelectionVersion: number;
}

export interface CalendarToday {
  meetings: { title: string; start: string; end: string; joinUrl: string | null }[];
  error: string | null;
  /** Calendars read through Composio, e.g. "Google Calendar". */
  sources: string[];
}

export interface VoiceSettings {
  enabled: boolean;
  wakeWord: boolean;
  speakAnswers: boolean;
  /** After a spoken answer, listen for a reply without the wake word. */
  conversation: boolean;
  /** Read suggestions aloud and take a spoken choice. */
  speakSuggestions: boolean;
  voice: string;
  speed: number;
  /** Which voice model the voice was picked for (3 = Supertonic 3). */
  model?: number;
}

export interface VoiceStatus {
  models: { id: string; label: string; size: number; installed: boolean }[];
  missingBytes: number;
  downloading: boolean;
  listening: boolean;
  error: string | null;
  voices: { id: string; label: string }[];
}

/** The first-run welcome line and when each part of it is heard. */
export interface WelcomeSpeech {
  /** The welcome step being spoken, and every step's line. */
  step: number;
  lines: string[];
  script: string;
  /** Sentences as queued: start (Unix ms) and length (ms). */
  pieces: { text: string; startsAt: number; ms: number }[];
  /** When the last word stops sounding, once known. */
  endsAt: number | null;
  /** Nothing will be heard; the welcome paces the words itself. */
  silent: boolean;
  /** Speech is on its way (the model may still be loading). */
  pending: boolean;
}

export interface VoiceHeard {
  text: string;
  final: boolean;
  byVoice: boolean;
}

export interface VoiceDownload {
  label: string;
  done: number;
  total: number;
  finished: boolean;
  error: string | null;
}

export const AI_PROVIDERS = ["claude_code", "codex", "anthropic", "local"] as const;
export type AiProviderId = (typeof AI_PROVIDERS)[number];

export interface AiSettings {
  order: AiProviderId[];
  claudeCode: { enabled: boolean; path: string; model: string };
  codex: { enabled: boolean; path: string; model: string };
  /** Who gets handoffs: "auto", "claude_code" or "codex". */
  codingAgent: string;
  local: { enabled: boolean; baseUrl: string; model: string; visionModel: string };
  anthropic: { enabled: boolean; model: string };
  semif: {
    enabled: boolean;
    command: string[];
    mode: string;
    backend: string;
    model: string;
    revision: string;
    gguf: string;
  };
  decisions: boolean;
}

export const PROVIDER_LABELS: Record<string, string> = {
  claude_code: "Claude Code",
  codex: "Codex",
  anthropic: "Anthropic API",
  local: "Local model",
  semif: "SemIf",
};

/** Where Sidekick listens for Claude Code hooks (ClaudeCodeSensor::DEFAULT_PORT). */
export const CLAUDE_HOOK_URL = "http://127.0.0.1:47821/claude-code";

export interface ProviderStatus {
  id: string;
  available: boolean;
  local: boolean;
}

export interface ChatMessage {
  role: "user" | "assistant";
  content: string;
}

/** One turn of an Ask conversation, as the island shows it. */
export interface Turn extends ChatMessage {
  provider?: string | null;
  error?: string | null;
  streaming?: boolean;
  /** A screenshot went with this question. */
  screen?: boolean;
  /** Why the local model suggests continuing in Claude Code. */
  handoff?: string | null;
  /** The tool the model is using right now. */
  tool?: string | null;
  /** Every tool step taken for this answer, in order (multi-step tasks). */
  steps?: string[];
  /** Actions offered as buttons; each runs on a tap. */
  proposals?: Proposal[];
}

export interface Proposal {
  id: string;
  label: string;
  /** Set once tapped: what happened, and Undo if it can be undone. */
  ran?: { ok: boolean; message: string; undoId: number | null; path: string | null; undone?: boolean };
}

export interface AskContext {
  app: string | null;
  title: string | null;
  clipboardKind: string | null;
  clipboardPreview: string | null;
  clipboardSecret: boolean;
}

export interface AskOpen {
  context: AskContext;
  prompt: string | null;
  ask: boolean;
  /** Attach the clipboard to the first question. */
  clipboard: boolean;
  /** Text of the web page the question is about. */
  page: string | null;
  /** Which island panel to show. */
  view?: "ask" | "settings" | "welcome";
  /** A tool to start with: "screen" or "clipboard". */
  tool?: "screen" | "clipboard" | null;
}

export interface SearchHit {
  source: string;
  reference: string;
  title: string;
  snippet: string;
  ts: string;
}

export interface McpInfo {
  url: string;
  token: string;
}

export interface AppTime {
  app: string;
  project: string;
  secs: number;
}

export function formatDuration(secs: number): string {
  const h = Math.floor(secs / 3600);
  const m = Math.round((secs % 3600) / 60);
  return h ? `${h} h ${m} min` : `${m} min`;
}

export interface BrowserInfo {
  token: string;
  port: number;
}

export const THEMES = ["pearl", "graphite", "midnight"] as const;
export type Theme = (typeof THEMES)[number];

export interface Suggestion {
  id: string;
  skillId: string;
  title: string;
  detail: string;
  options: string[];
  /** Which options can become "Always do this". */
  always?: boolean[];
}

export interface PasswordEditDraft {
  id: string;
  domain: string;
  username: string;
  password: string;
}

export interface PasswordSaved {
  id: string;
  domain: string;
  expiresAt: number;
  message: string;
}

export interface PasswordWriteResult {
  browser: string;
  status: "saved" | "unchanged" | "locked" | "conflict" | "unsupported" | "failed" | "disabled" | "cancelled";
  message: string;
}

export interface PasswordPrompt {
  id: string;
  domain: string;
  username: string;
  seconds: number;
  phase: "queued" | "countdown" | "editing" | "retry" | "writing" | "saved";
  missing: string[];
  conflicts: string[];
  existing: string[];
  unavailable: PasswordWriteResult[];
  sources: string[];
  mirror: boolean;
}

export interface PasswordMirrorStatus {
  running: boolean;
  message: string;
}

export interface PasswordBrowserInfo {
  id: string;
  name: string;
  enabled: boolean;
}

export interface StoredEvent {
  id: string;
  ts: string;
  kind: string;
  source: string;
  payload: unknown;
  sensitivity: string;
}

export interface AppInfo {
  version: string;
  dbPath: string;
  settingsPath: string;
  eventCount: number;
}

export interface HitRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export const DEFAULT_SETTINGS: Settings = {
  muted: false,
  masterVolume: 0.6,
  cueVolumes: {},
  collapseAfterSecs: 8,
  launchAtLogin: true,
  sensors: {},
  pause: { kind: "none" },
  theme: "pearl",
  soundKit: "01",
  paletteHotkey: "Ctrl+Space",
  shortcuts: {
    talk: "Ctrl+Alt+Space",
    accept: "Ctrl+Alt+Enter",
    dismiss: "Ctrl+Alt+Backspace",
    screen: "Ctrl+Alt+S",
    clipboard: "Ctrl+Alt+V",
    pause: "Ctrl+Alt+P",
    settings: "Ctrl+Alt+Comma",
  },
  codeFolders: [],
  indexFolders: [],
  endOfDayHour: 18,
  ai: {
    order: ["claude_code", "codex", "anthropic", "local"],
    claudeCode: { enabled: true, path: "", model: "" },
    codex: { enabled: true, path: "", model: "" },
    codingAgent: "auto",
    local: { enabled: true, baseUrl: "http://localhost:11434/v1", model: "", visionModel: "" },
    anthropic: { enabled: true, model: "" },
    semif: {
      enabled: true,
      command: ["semif-score"],
      mode: "direct",
      backend: "llamacpp",
      model: "openbmb/MiniCPM5-2B",
      revision: "main",
      gguf: "",
    },
    decisions: true,
  },
  calendar: { remindMinutes: 5 },
  semanticSearch: { enabled: true, model: "nomic-embed-text" },
  onboarded: false,
  welcomeStep: 0,
  checkUpdates: true,
  composio: { enabled: false, account: "", userId: "", url: "", headers: {} },
  denyApps: ["1password.exe", "bitwarden.exe", "keepass.exe", "keepassxc.exe"],
  denySites: [],
  routines: true,
  routinesAuto: false,
  hideInFullscreen: false,
  notifications: { enabled: true, apps: {}, vip: [] },
  recipes: [],
  memory: [],
  agent: { ask: "outward", places: {} },
  passwordBrowsers: null,
  passwordSelectionVersion: 1,
  voice: {
    enabled: true,
    wakeWord: true,
    speakAnswers: true,
    conversation: true,
    speakSuggestions: true,
    voice: "f5",
    speed: 1,
  },
};

export const SENSOR_IDS = [
  { id: "calendar", label: "Calendar", hint: "Reads your calendar through Composio for meeting reminders." },
  { id: "downloads", label: "Downloads", hint: "Notices finished downloads the moment they land." },
  { id: "screenshots", label: "Screenshots", hint: "Notices new screenshots in Pictures\\Screenshots." },
  { id: "ports", label: "Dev servers", hint: "Notices local servers starting, checked every second." },
  { id: "clipboard", label: "Clipboard", hint: "Notices copied text. Secrets are never stored." },
  { id: "window", label: "Active window", hint: "Knows which app is in front; hides the island in fullscreen." },
  {
    id: "claude_code",
    label: "Claude Code",
    hint: "Hears when a Claude Code session finishes or needs you (add the hook under AI).",
  },
  {
    id: "browser",
    label: "Browser",
    hint: "Hears the Sidekick extension: sign-in pages, long reads, Upwork jobs, too many tabs.",
  },
  { id: "system", label: "Disk and memory", hint: "Warns when a drive is almost full or memory stays high." },
  {
    id: "idle",
    label: "Away detection",
    hint: "Holds suggestions while you are away and shows them when you are back.",
  },
] as const;

export interface ActionResult {
  ok: boolean;
  message: string;
  path: string | null;
  auto: boolean;
  /** Set when the action created a file that Undo can move to the bin. */
  undoId: number | null;
}

export interface SkillInfo {
  id: string;
  name: string;
  description: string;
  event: string;
  enabled: boolean;
  auto: boolean;
  autoByDefault: boolean;
  /** Quiet after Not now three times, until this time. */
  mutedUntil: string | null;
}

export interface CapabilityInfo {
  found: string[];
  skillsDir: string;
  skillErrors: string[];
}

export interface ActionRecord {
  id: number;
  ts: string;
  skillId: string;
  action: string;
  label: string;
  ok: boolean;
  message: string;
  auto: boolean;
  undoPath: string | null;
  undone: boolean;
}

/** Undo is kept for 24 hours (FR-ACT-04). */
export function canUndo(a: ActionRecord, now = Date.now()): boolean {
  return !!a.undoPath && !a.undone && now - Date.parse(a.ts) < 24 * 60 * 60 * 1000;
}

export function isPaused(pause: Pause, now = Date.now()): boolean {
  if (pause.kind === "indefinite") return true;
  if (pause.kind === "until") return Date.parse(pause.until) > now;
  return false;
}

export type SetupGroup = "ai" | "connect" | "tools";

export interface SetupItem {
  id: string;
  group: SetupGroup;
  title: string;
  why: string;
  done: boolean;
  status: string;
  command: string | null;
  runnable: boolean;
  tab: string | null;
  recommended: boolean;
}

export interface SetupStatus {
  items: SetupItem[];
  installAll: string | null;
}

export interface ComposioSettings {
  enabled: boolean;
  /** Who signed in (their email). */
  account: string;
  userId: string;
  /** Another MCP link; empty means Composio Connect. */
  url: string;
  headers: Record<string, string>;
}

export interface ComposioApp {
  slug: string;
  name: string;
  /** What Sidekick uses it for; empty for other apps in the account. */
  why: string;
  connected: boolean;
}

export interface ComposioStatus {
  signedIn: boolean;
  account: string;
  apps: ComposioApp[];
  error: string | null;
}

export interface BrowserStatus {
  id: string;
  name: string;
  connected: boolean;
}

export interface ExtensionGuide {
  copied: string;
  /** The extensions page, to paste into the address bar. */
  page: string;
  steps: string[];
}

export interface Folder {
  path: string;
  label: string;
  repos: number;
}

export interface Found {
  codeFolders: Folder[];
  searchFolders: Folder[];
  chatModels: string[];
  embedModels: string[];
  claudeInstalled: boolean;
  claudeHooks: boolean;
  claudeMcp: boolean;
  composioSignedIn: boolean;
  composioInClaude: boolean;
  browsers: string[];
  installable: SetupItem[];
}

/** Older names used by the welcome and setup guides. */
export type DetectedFolder = Folder;
export type SetupFound = Found;

export interface SetupPlan {
  codeFolders: string[];
  searchFolders: string[];
  chatModel: string | null;
  claudeHooks: boolean;
  claudeMcp: boolean;
  install: string[];
  voice: boolean;
  launchAtLogin: boolean;
}

export interface LocalModels {
  reachable: boolean;
  chat: string[];
  embed: string[];
}

export interface LaterItem {
  id: string;
  title: string;
  detail: string;
  minutesAgo: number;
  /** Shown and timed out while you were away, not held back. */
  missed: boolean;
}

export interface ChatSummary {
  id: string;
  title: string;
  updated: string;
}

/** Shortcut actions besides Ask, in the order Settings shows them. */
export const SHORTCUT_ACTIONS: { id: string; label: string }[] = [
  { id: "talk", label: "Talk" },
  { id: "accept", label: "Accept the suggestion" },
  { id: "dismiss", label: "Stop or Not now" },
  { id: "screen", label: "Ask about the screen" },
  { id: "clipboard", label: "Clipboard history" },
  { id: "pause", label: "Pause or resume" },
  { id: "settings", label: "Settings" },
];

export interface ComposioCheck {
  tools: number;
  reads: number;
  sample: string[];
}

/** Something opened early on most mornings (see Settings > Privacy). */
export interface RoutineItem {
  kind: "app" | "site";
  key: string;
  label: string;
  target: string;
  browser: string;
  /** Of the last five matching days. */
  days: number;
}

/** Coding agents on this PC, and which one gets handoffs. */
export interface Agents {
  claudeCode: boolean;
  codex: boolean;
  /** "Claude Code" or "Codex", or null when neither is installed. */
  handoff: string | null;
}

/** How much a notification interrupts. */
export type NotifyLevel = "now" | "soon" | "digest" | "never";

export interface NotificationSettings {
  enabled: boolean;
  /** The user's level per app; missing means Sidekick decides. */
  apps: Record<string, NotifyLevel>;
  /** People whose messages always come through right away. */
  vip: string[];
}

/** One notification Sidekick read, and where it put it. */
export interface InboxItem {
  id: number;
  app: string;
  title: string;
  body: string;
  ts: string;
  level: NotifyLevel;
  why: string;
  code?: string | null;
}

export interface InboxStatus {
  /** Reading works (Windows only). */
  readable: boolean;
  error?: string | null;
  items: InboxItem[];
  /** Apps seen so far, newest first, with Sidekick's level for each. */
  apps: { app: string; level: NotifyLevel | null; count: number }[];
}

/** When a recipe runs. */
export type Trigger =
  | { when: "manual" }
  | { when: "time"; time: string; days: string[] }
  | { when: "notification"; app: string; contains: string }
  | { when: "download"; kind: string }
  | { when: "meeting_ended" }
  | { when: "app_opened"; app: string };

/** A saved task: what to do, in the user's words, and when. */
export interface Recipe {
  id: string;
  name: string;
  prompt: string;
  trigger: Trigger;
  /** Start without asking first; sending still waits for a tap. */
  auto: boolean;
  enabled: boolean;
}

/** When an agent step waits for a tap. */
export type AgentAsk = "each" | "outward" | "irreversible";
export type PlaceRule = "allow" | "ask" | "never";
export interface AgentSettings {
  ask: AgentAsk;
  /** Per app or site ("whatsapp", "mail.google.com"). */
  places: Record<string, PlaceRule>;
}
