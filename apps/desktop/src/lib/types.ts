import { FAST_CLAUDE_MODEL, FAST_CODEX_MODEL } from "./ai-models";

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
  "focus",
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

/** One MCP server started as a program on this PC (no shell). */
export interface DirectMcpServer {
  id: string;
  name: string;
  command: string;
  args: string[];
}

export interface DirectMcp {
  servers: DirectMcpServer[];
}

export interface Settings {
  muted: boolean;
  masterVolume: number;
  cueVolumes: Record<string, number>;
  collapseAfterSecs: number;
  launchAtLogin: boolean;
  sensors: Record<string, boolean>;
  pause: Pause;
  theme: Theme;
  /** An editor id, or "auto" for the one used most this week. */
  codeEditor: string;
  islandColor: IslandColor;
  soundKit: string;
  skills?: Record<string, { enabled?: boolean | null; auto?: boolean | null }>;
  /** Ask shortcut. A leading `-` turns it off without clearing the combo. */
  paletteHotkey: string;
  /** Global shortcuts by action. A leading `-` turns one off without clearing. */
  shortcuts: Record<string, string>;
  codeFolders: string[];
  savedProjects: string[];
  indexFolders: string[];
  endOfDayHour: number;
  /** Where clones go, by owner. */
  cloneRules?: Record<string, string>;
  ai: AiSettings;
  voice: VoiceSettings;
  calendar: { remindMinutes: number };
  semanticSearch: { enabled: boolean; model: string };
  /** MCP servers connected directly (stdio), preferred over Composio. */
  directMcp: DirectMcp;
  onboarded: boolean;
  /** 0-based welcome step; resumed until onboarded is true. */
  welcomeStep: number;
  checkUpdates: boolean;
  composio: ComposioSettings;
  /** Apps connected directly over MCP. */
  directServers: DirectServer[];
  denyApps: string[];
  denySites: string[];
  routines: boolean;
  routinesAuto: boolean;
  /** The mascot idles on its own: glances around, blinks, the odd smile. */
  alive: boolean;
  /** After a crash, offer a report to send (never sent on its own). */
  crashReports: boolean;
  /** Fade the island while a fullscreen app is in front. */
  hideInFullscreen: boolean;
  tips: boolean;
  notifications: NotificationSettings;
  recipes: Recipe[];
  memory: string[];
  userName: string;
  people: string;
  projects: string;
  answerStyle: string;
  codes: boolean | null;
  assistantName: string;
  learning: boolean;
  agent: AgentSettings;
}

export interface CalendarToday {
  meetings: {
    title: string;
    start: string;
    end: string;
    joinUrl: string | null;
    /** The page or file used last time for this meeting, from history. */
    notes?: { title: string; source: string; reference: string } | null;
  }[];
  error: string | null;
  /** Calendars read through Composio, e.g. "Google Calendar". */
  sources: string[];
}

export interface VoiceSettings {
  enabled: boolean;
  wakeWord: boolean;
  speakAnswers: boolean;
  /** While you talk: one slim line, or a waveform and larger words. */
  listeningStyle: "compact" | "full";
  /** After a spoken answer, listen for a reply without the wake word. */
  conversation: boolean;
  /** Read suggestions aloud and take a spoken choice. */
  speakSuggestions: boolean;
  /** Talking over a spoken answer stops it and listens. */
  interrupt: boolean;
  voice: string;
  speed: number;
  /** Words voice gets wrong: "horsepot = hotspot, sidekik = Duskie". */
  fixes?: string;
  /** Which voice model the voice was picked for (3 = Supertonic 3). */
  model?: number;
  /** Start answering at a pause in speech. Off by default. */
  live?: boolean;
  /** Keep Live conversations in Ask history, as text. */
  liveHistory?: boolean;
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
  /** The words settled for a moment: an answer may start early. */
  pause?: boolean;
  byVoice: boolean;
}

export interface VoiceDownload {
  label: string;
  done: number;
  total: number;
  finished: boolean;
  error: string | null;
}

export const AI_PROVIDERS = [
  "local",
  "gemini",
  "groq",
  "claude_code",
  "codex",
  "anthropic",
  "openrouter",
  "openai",
  "xai",
] as const;
/** Cloud models reached with an API key kept in Credential Manager. */
export type CloudId = "gemini" | "groq" | "openrouter" | "openai" | "xai";
export const CLOUD_IDS: CloudId[] = ["gemini", "groq", "openrouter", "openai", "xai"];
export type AiProviderId = (typeof AI_PROVIDERS)[number];

export interface AiSettings {
  order: AiProviderId[];
  claudeCode: { enabled: boolean; path: string; model: string };
  codex: { enabled: boolean; path: string; model: string };
  /** Who gets handoffs: "auto", "claude_code" or "codex". */
  codingAgent: string;
  local: { enabled: boolean; baseUrl: string; model: string; visionModel: string };
  anthropic: { enabled: boolean; model: string };
  gemini: { enabled: boolean; model: string };
  groq: { enabled: boolean; model: string };
  openrouter: { enabled: boolean; model: string };
  openai: { enabled: boolean; model: string };
  xai: { enabled: boolean; model: string };
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
  /** New Ask chats start with This PC only on. */
  localOnly: boolean;
}

export const PROVIDER_LABELS: Record<string, string> = {
  claude_code: "Claude Code",
  codex: "Codex",
  anthropic: "Anthropic API",
  local: "Local model",
  gemini: "Gemini",
  groq: "Groq",
  openrouter: "OpenRouter",
  openai: "OpenAI",
  xai: "xAI Grok",
  semif: "SemIf",
};

/** Where Duskie listens for Claude Code hooks (ClaudeCodeSensor::DEFAULT_PORT). */

export interface ProviderStatus {
  id: string;
  available: boolean;
  local: boolean;
  /** Installed / keyed / model chosen, even if not running right now. */
  configured: boolean;
}

export interface ChatMessage {
  role: "user" | "assistant";
  content: string;
}

/** One turn of an Ask conversation, as the island shows it. */
export interface Turn extends ChatMessage {
  provider?: string | null;
  /** Where app tools answered from ("direct connection", "Composio"). */
  sources?: string[];
  error?: string | null;
  streaming?: boolean;
  /** A screenshot went with this question. */
  screen?: boolean;
  /** This question was asked with This PC only on; a reopened chat keeps it. */
  localOnly?: boolean;
  /** Why the local model suggests continuing in Claude Code. */
  handoff?: string | null;
  /** The tool the model is using right now. */
  tool?: string | null;
  /** Every tool step taken for this answer, in order (multi-step tasks). */
  steps?: string[];
  /** Actions offered as buttons; each runs on a tap. */
  proposals?: Proposal[];
  /** What the answer cost in US dollars (Anthropic API only). */
  cost?: number;
  /** When the question was sent (ms since 1970), and how long the answer took. */
  startedAt?: number;
  tookMs?: number;
  /** How long until the first word showed. */
  firstMs?: number;
  /** Asked while offline: web answers wait for the connection. */
  offline?: boolean;
}

export interface Proposal {
  id: string;
  label: string;
  /** One step of a task: shown with the others as a plan, run in order. */
  step?: boolean;
  /** Undo can put this back once it has run. */
  undoable?: boolean;
  /** Set once tapped: what happened, and Undo if it can be undone. */
  ran?: { ok: boolean; message: string; undoId: number | null; path: string | null; undone?: boolean };
}

export interface SuggestionRate {
  skill: string;
  taken: number;
  dismissed: number;
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
  /** When the open was asked for (ms since 1970), for the timings. */
  sentAt?: number;
  /** Which Settings tab to show with view "settings". */
  settingsTab?: string | null;
  /** Opens Agents with a new session in this project. */
  project?: string | null;
  /** Ask panel tab to show. */
  tab?: "ask" | "agents" | null;
  /** Rust open generation; stale opens after a close are ignored. */
  panelGen?: number;
}

/** One measured moment, for the timings overlay. */
export interface Timing {
  name: "open_to_ready" | "enter_to_first_word" | "speech_to_first_sound";
  ms: number;
}

export interface SearchHit {
  source: string;
  reference: string;
  title: string;
  snippet: string;
  ts: string;
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

export const THEMES = ["atelier", "pearl", "aurora", "chrome", "peach", "mint", "lilac", "onyx"] as const;
export type Theme = (typeof THEMES)[number];
export interface EditorList {
  editors: { id: string; name: string; minutes: number }[];
  /** The editor projects open in now. */
  current: string | null;
}

export const ISLAND_COLORS = [
  "atelier",
  "solid_black",
  "black_glass",
  "graphite",
  "midnight",
  "smoke",
  "warm_graphite",
] as const;
export type IslandColor = (typeof ISLAND_COLORS)[number];

export interface Suggestion {
  id: string;
  skillId: string;
  title: string;
  detail: string;
  options: string[];
  /** Action id per option (parallel to options), for Opening… feedback. */
  actions: string[];
  /** Which options can become "Always do this". */
  always?: boolean[];
  /** Why this showed, from how often you took this kind before. */
  why?: string;
  /** A small pill that opens into the card on hover or while Alt is held. */
  compact?: boolean;
  /** Shows without a sound. */
  silent?: boolean;
}

/** A newer Duskie release than the one running. */
export interface UpdateInfo {
  version: string;
  current: string;
  url: string;
}

export interface UpdateProgress {
  phase: "preparing" | "downloading" | "verifying" | "opening" | "failed";
  done: number;
  total: number | null;
  error: string | null;
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
  collapseAfterSecs: 6,
  launchAtLogin: true,
  sensors: {},
  pause: { kind: "none" },
  theme: "atelier",
  codeEditor: "auto",
  islandColor: "atelier",
  soundKit: "sidekick",
  paletteHotkey: "Ctrl+Space",
  shortcuts: {
    agents: "Ctrl+Shift+Space",
    talk: "Ctrl+Alt+Space",
    accept: "Ctrl+Alt+Enter",
    dismiss: "Ctrl+Alt+Backspace",
    screen: "Ctrl+Alt+S",
    clipboard: "Ctrl+Alt+V",
    focus: "Ctrl+Alt+F",
    pause: "Ctrl+Alt+P",
    settings: "Ctrl+Alt+Comma",
  },
  codeFolders: [],
  savedProjects: [],
  indexFolders: [],
  endOfDayHour: 18,
  cloneRules: {},
  ai: {
    order: ["local", "gemini", "groq", "claude_code", "codex", "anthropic", "openrouter", "openai", "xai"],
    claudeCode: { enabled: true, path: "", model: FAST_CLAUDE_MODEL },
    codex: { enabled: true, path: "", model: FAST_CODEX_MODEL },
    codingAgent: "auto",
    local: { enabled: true, baseUrl: "http://127.0.0.1:11434/v1", model: "", visionModel: "" },
    anthropic: { enabled: true, model: FAST_CLAUDE_MODEL },
    gemini: { enabled: false, model: "" },
    groq: { enabled: false, model: "" },
    openrouter: { enabled: false, model: "" },
    openai: { enabled: false, model: "" },
    xai: { enabled: false, model: "" },
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
    localOnly: false,
  },
  calendar: { remindMinutes: 5 },
  semanticSearch: { enabled: true, model: "nomic-embed-text" },
  directMcp: { servers: [] },
  onboarded: false,
  welcomeStep: 0,
  checkUpdates: true,
  composio: { enabled: false, account: "", userId: "", url: "", headers: {} },
  directServers: [],
  denyApps: ["1password.exe", "bitwarden.exe", "keepass.exe", "keepassxc.exe"],
  denySites: [],
  routines: true,
  routinesAuto: false,
  hideInFullscreen: true,
  tips: true,
  alive: true,
  crashReports: false,
  notifications: {
    enabled: true,
    apps: {},
    vip: [],
    profile: "normal",
    lessons: { entries: {} },
    imap: { host: "", port: 993, user: "" },
  },
  recipes: [],
  memory: [],
  userName: "",
  people: "",
  projects: "",
  answerStyle: "",
  codes: null,
  assistantName: "Duskie",
  learning: true,
  agent: { ask: "outward", places: {} },
  voice: {
    enabled: true,
    wakeWord: true,
    speakAnswers: true,
    listeningStyle: "compact",
    conversation: true,
    speakSuggestions: true,
    interrupt: true,
    live: false,
    liveHistory: true,
    voice: "f4",
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
    hint: "Hears the Duskie extension: long reads, Upwork jobs, too many tabs.",
  },
  { id: "system", label: "Disk and memory", hint: "Warns when a drive is almost full or memory stays high." },
  {
    id: "idle",
    label: "Away detection",
    hint: "Holds suggestions while you are away and shows them when you are back.",
  },
] as const;

/** One file the branch update left clashing. */
export interface Clash {
  file: string;
  spots: number;
  mine: string;
  theirs: string;
}

/** An update of a feature branch from main, stopped on conflicts. */
export interface MergeState {
  branch: string;
  main: string;
  stashed: boolean;
  files: Clash[];
}

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

export type SkillOrigin = "claude" | "claude_plugin" | "codex" | "duskie" | "url";
export type SkillTarget = "claude" | "codex" | "local";

/** A skill found on this PC that is not in the library yet. */
export interface FoundSkill {
  name: string;
  description: string;
  origin: SkillOrigin;
  dir: string;
  hash: string;
  files: string[];
  hasScripts: boolean;
}

/** A skill in the library and where it is written. */
export interface LibrarySkill {
  name: string;
  description: string;
  origin: SkillOrigin;
  hash: string;
  hasScripts: boolean;
  targets: Partial<Record<SkillTarget, boolean>>;
  synced: Partial<Record<SkillTarget, string>>;
  url: string | null;
  commit: string | null;
}

/** One skill behind a link, compared with the library. */
export interface LinkSkill {
  name: string;
  description: string;
  files: number;
  bytes: number;
  hasScripts: boolean;
  status: "new" | "changed" | "same";
  /** Files added, changed or removed against the library copy. */
  diff: string[];
}

export interface LinkPreview {
  url: string;
  commit: string;
  skills: LinkSkill[];
}

export interface SyncReport {
  written: string[];
  conflicts: string[];
  backups: string[];
}

/** One file or folder in the editor's project tree. */
export interface TreeEntry {
  path: string;
  dir: boolean;
}

export interface OpenFile {
  text: string;
  /** What was on disk when opened; a save sends it back. */
  stamp: string;
}

export interface EditorHit {
  path: string;
  line: number;
  text: string;
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

/** Undo is kept for 24 hours. */
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
  /** Button label for running it: "Install", "Download", "Sign in", "Run". */
  action: string;
  /** "Run" opens the installed app instead of a terminal window. */
  opensApp: boolean;
  /** Opens a terminal and copies the command for the user to paste. */
  opensTerminal: boolean;
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
  /** What Duskie uses it for; empty for other apps in the account. */
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
  vision: string[];
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
  { id: "agents", label: "Agents" },
  { id: "talk", label: "Talk" },
  { id: "accept", label: "Accept the suggestion" },
  { id: "dismiss", label: "Stop or Not now" },
  { id: "screen", label: "Ask about the screen" },
  { id: "clipboard", label: "Clipboard history" },
  { id: "focus", label: "Focus on or off" },
  { id: "pause", label: "Pause or resume" },
  { id: "settings", label: "Settings" },
];

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
/** How much an agent may do without asking. */
export type AgentMode = "plan" | "ask" | "edit" | "full";

export interface AgentStarted {
  id: string;
  agent: string;
  project: string;
  branch: string | null;
  /** Changes can be reviewed and undone (the project uses git). */
  reviewable: boolean;
  /** Its own worktree's branch, when another session was in this repo. */
  worktree?: string | null;
}

export interface FileChange {
  path: string;
  status: "added" | "deleted" | "modified";
  added: number;
  removed: number;
  hunks: { header: string; lines: string[] }[];
}

export interface Agents {
  claudeCode: boolean;
  codex: boolean;
  copilot: boolean;
  cursor: boolean;
  /** The agent that gets handoffs by name, or null when none is installed. */
  handoff: string | null;
  /** Every agent with whether it is ready, for the picker. */
  list?: AgentInfo[];
}

/** A chat started in Cursor's own window, watched from Agents. */
export interface CursorChat {
  id: string;
  title: string;
  path: string | null;
  project: string;
  updatedAt: number;
  status: "working" | "idle";
  lastReply: string;
}

/** One agent in the picker. */
export interface AgentInfo {
  id: string;
  name: string;
  installed: boolean;
  /** null when Duskie cannot tell. */
  signedIn: boolean | null;
  /** Out of plan usage for now. */
  limited: boolean;
  /** The one step that makes it ready. */
  fix: string | null;
  /** Runs on this PC; nothing leaves it. */
  local?: boolean;
}

/** One open pull request in the Repos list. */
export interface RepoPr {
  number: number;
  title: string;
  url: string;
  branch: string;
  mine: boolean;
  ci: "pass" | "fail" | "pending" | "none";
  /** The first failing check and its link. */
  failing: [string, string] | null;
  reviewRequested: boolean;
}

/** One repo in the Repos list. */
export interface RepoRow {
  name: string;
  path: string;
  slug: string | null;
  branch: string;
  ahead: number;
  behind: number;
  changed: number;
  prs: RepoPr[];
  ci: RepoPr["ci"];
  reviews: number;
}

export interface ReposOverview {
  /** How GitHub is reached: the gh sign-in, Composio, or not at all. */
  via: "gh" | "composio" | "none";
  repos: RepoRow[];
}

/** How much a notification interrupts. */
export type NotifyLevel = "now" | "soon" | "digest" | "never";

export interface NotificationSettings {
  enabled: boolean;
  /** The user's level per app; missing means Duskie decides. */
  apps: Record<string, NotifyLevel>;
  /** People whose messages always come through right away. */
  vip: string[];
  /** Normal (soft sound for quieter things) or Calm (sound only for Urgent). */
  profile: AttentionProfile;
  /** What Duskie learned from what you act on and ask for less of. */
  lessons: { entries: Record<string, { acted: number; showLess: number; updated: number }> };
  /** An IMAP account (Advanced). The password is in the keychain. */
  imap: { host: string; port: number; user: string };
}

export type AttentionProfile = "normal" | "calm";

/** An app connected directly over MCP. */
export interface DirectServer {
  id: string;
  app: string;
  kind: "stdio" | "url";
  command: string;
  args: string[];
  /** Names only; values are in the keychain. */
  envNames: string[];
  url: string;
  /** The exact command was confirmed by the user. */
  confirmed: boolean;
  writesAllowed: string[];
}

/** A provider or connector paused after repeated errors. */
export interface LimitState {
  name: string;
  /** Seconds until it is tried again. */
  pausedSecs: number;
}

/** A tool a direct server offers. Writes stay off until allowed. */
export interface ServerTool {
  name: string;
  description: string;
  writes: boolean;
  allowed: boolean;
}

/** Where the Gmail connection stands. Never holds message content. */
export type MailState = "disconnected" | "connected" | "offline" | "auth_expired";

export interface MailStatus {
  state: MailState;
  /** False when this build has no Google sign-in set up. */
  configured: boolean;
  /** Seconds since the epoch of the last good check. */
  lastOk: number | null;
  error: string | null;
  /** Which account is connected. */
  provider: "gmail" | "imap" | "composio" | null;
}

/** One changed file in the Git dock. */
export interface GitFile {
  path: string;
  /** M changed, A added, D deleted, R renamed, ? new, U conflict. */
  kind: string;
  staged: boolean;
  unstaged: boolean;
}

/** One commit in the Git dock's History tab. */
export interface CommitRow {
  hash: string;
  subject: string;
  author: string;
  /** Relative time, as git words it. */
  when: string;
}

/** One stash in the Git dock's Stashes tab. */
export interface StashRow {
  /** stash@{0} and so on. */
  name: string;
  message: string;
  when: string;
}

/** The capped diff a model reads to write a commit message. */
export interface MessageDiff {
  text: string;
  /** Files the text covers, and how many changed in all. */
  covered: number;
  total: number;
}

/** One learned sender or source, as Settings lists it. */
export interface LessonRow {
  key: string;
  /** Points added to or taken from the score; negative means less. */
  shift: number;
  updated: number;
}

/** One notification Duskie read, and where it put it. */
export interface InboxItem {
  id: number;
  app: string;
  title: string;
  body: string;
  ts: string;
  level: NotifyLevel;
  why: string;
  code?: string | null;
  /** Mirrored from the phone (Phone Link). */
  phone?: boolean;
  /** Other apps that brought the same thing. */
  also?: string[];
}

export interface InboxStatus {
  /** Reading works (Windows only). */
  readable: boolean;
  error?: string | null;
  items: InboxItem[];
  /** Apps seen so far, newest first, with Duskie's level for each. */
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

/** Ask's instant results: apps and files named like what is typed, no AI. */
export interface InstantResults {
  apps: { name: string; id: string; minutes: number; browser?: string | null }[];
  files: { name: string; path: string; folder: boolean; place: string }[];
  /** "18% of 2450" worked out, when the text is math. */
  calc?: string | null;
}

/** Something Duskie learned, for Settings > Memory. */
export interface Learned {
  kind: "choice" | "quiet" | "routine";
  key: string;
  label: string;
  text: string;
  why: string;
}
export interface Activity {
  id: string;
  source: string;
  parentId: string | null;
  label: string;
  phase:
    | "running"
    | "downloading"
    | "verifying"
    | "installing"
    | "awaitingUser"
    | "completed"
    | "failed"
    | "unconfirmed";
  startedAt: number;
  lastProgressAt: number;
  transferred: number | null;
  total: number | null;
  outcome: string | null;
  dismissed: boolean;
  revision: number;
  destination: string | null;
}
export interface ActivitySnapshot {
  revision: number;
  activities: Activity[];
}
export interface AgentReviewTurn {
  id: string;
  entry: number;
  files: FileChange[];
  decisions: Record<string, "accepted" | "rejected">;
  error: string | null;
}
