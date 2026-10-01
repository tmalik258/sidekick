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

export const CUES = ["yawn", "chirp", "pop", "open", "ding", "boop"] as const;

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
}

export const THEMES = ["pearl", "graphite", "midnight"] as const;
export type Theme = (typeof THEMES)[number];

export interface Suggestion {
  id: string;
  skillId: string;
  title: string;
  detail: string;
  options: string[];
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
  launchAtLogin: false,
  sensors: {},
  pause: { kind: "none" },
  theme: "pearl",
  soundKit: "01",
};

export const SENSOR_IDS = [
  { id: "downloads", label: "Downloads", hint: "Notices finished downloads the moment they land." },
  { id: "ports", label: "Dev servers", hint: "Notices local servers starting, checked every second." },
  { id: "clipboard", label: "Clipboard", hint: "Notices copied text. Secrets are never stored." },
  { id: "window", label: "Active window", hint: "Knows which app is in front; hides the island in fullscreen." },
  { id: "heartbeat", label: "Heartbeat (debug)", hint: "A test event every 30 seconds." },
] as const;

export interface ActionResult {
  ok: boolean;
  message: string;
  path: string | null;
  auto: boolean;
}

export interface SkillInfo {
  id: string;
  name: string;
  description: string;
  event: string;
  enabled: boolean;
  auto: boolean;
  autoByDefault: boolean;
}

export interface CapabilityInfo {
  found: string[];
  skillsDir: string;
  skillErrors: string[];
}

export interface ActionRecord {
  ts: string;
  skillId: string;
  action: string;
  label: string;
  ok: boolean;
  message: string;
  auto: boolean;
}

export function isPaused(pause: Pause, now = Date.now()): boolean {
  if (pause.kind === "indefinite") return true;
  if (pause.kind === "until") return Date.parse(pause.until) > now;
  return false;
}
