use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

/// Whether sensors are paused (FR-SET-01).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", content = "until", rename_all = "snake_case")]
pub enum Pause {
    #[default]
    None,
    Until(DateTime<Utc>),
    Indefinite,
}

impl Pause {
    pub fn for_minutes(minutes: u32, now: DateTime<Utc>) -> Self {
        Pause::Until(now + Duration::minutes(i64::from(minutes)))
    }

    pub fn is_active(&self, now: DateTime<Utc>) -> bool {
        match self {
            Pause::None => false,
            Pause::Until(until) => *until > now,
            Pause::Indefinite => true,
        }
    }
}

/// Shortcuts besides Ask, with their defaults.
pub const SHORTCUTS: &[(&str, &str)] = &[
    ("talk", "Ctrl+Alt+Space"),
    ("accept", "Ctrl+Alt+Enter"),
    ("dismiss", "Ctrl+Alt+Backspace"),
    ("screen", "Ctrl+Alt+S"),
    ("clipboard", "Ctrl+Alt+V"),
    ("pause", "Ctrl+Alt+P"),
    ("settings", "Ctrl+Alt+Comma"),
];

pub fn default_shortcuts() -> BTreeMap<String, String> {
    SHORTCUTS
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub muted: bool,
    /// 0.0 to 1.0.
    pub master_volume: f32,
    /// Per-cue volume, 0.0 to 1.0, keyed by cue name. Missing means 1.0.
    pub cue_volumes: BTreeMap<String, f32>,
    /// Seconds before an expanded island collapses with no interaction.
    pub collapse_after_secs: u32,
    pub launch_at_login: bool,
    /// Enabled flag per sensor id. Missing means enabled.
    pub sensors: BTreeMap<String, bool>,
    pub pause: Pause,
    /// Orb appearance: one of [`THEMES`].
    pub theme: String,
    /// UI sound kit: one of [`SOUND_KITS`].
    pub sound_kit: String,
    /// Per-skill switches set by the user (FR-SKL-06).
    pub skills: BTreeMap<String, SkillPref>,
    /// Global shortcut that turns the island into Ask mode (FR-UI-07).
    pub palette_hotkey: String,
    /// More global shortcuts, by action (talk, accept, dismiss, screen,
    /// clipboard, pause, settings). Empty turns one off.
    pub shortcuts: BTreeMap<String, String>,
    /// Folders with git repos to check at the end of the day (FR-DEV-09).
    /// Empty means the usual places (code, projects, source/repos, ...).
    pub code_folders: Vec<String>,
    /// Local hour after which unsaved work is reported.
    pub end_of_day_hour: u32,
    /// Folders whose text files are searchable (opt in, FR-RAG-05).
    pub index_folders: Vec<String>,
    pub ai: AiSettings,
    pub voice: VoiceSettings,
    pub calendar: CalendarSettings,
    /// Composio's MCP server, for apps like Jira, Slack and Gmail in chat.
    pub composio: ComposioSettings,
    pub semantic_search: SemanticSearch,
    /// The first-run welcome was finished or skipped.
    pub onboarded: bool,
    /// Look for a newer release once a day.
    pub check_updates: bool,
    /// Programs whose windows and copies Sidekick ignores (FR-SET-02).
    pub deny_apps: Vec<String>,
    /// Sites (and their subdomains) Sidekick ignores.
    pub deny_sites: Vec<String>,
}

/// Password managers are ignored from the start (FR-RAG-08).
pub const DEFAULT_DENY_APPS: &[&str] = &[
    "1password.exe",
    "bitwarden.exe",
    "keepass.exe",
    "keepassxc.exe",
    "lastpass.exe",
    "dashlane.exe",
    "enpass.exe",
    "proton pass.exe",
];

/// Search by meaning with an embedding model on this PC (FR-RAG-03).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SemanticSearch {
    pub enabled: bool,
    /// Embedding model on the local server, e.g. `nomic-embed-text`.
    pub model: String,
}

impl Default for SemanticSearch {
    fn default() -> Self {
        Self {
            enabled: true,
            model: "nomic-embed-text".into(),
        }
    }
}

/// Meeting reminders (FR-COMM-02); meetings come from Composio.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CalendarSettings {
    /// Minutes before a meeting to offer Join and Prep.
    pub remind_minutes: u32,
}

impl Default for CalendarSettings {
    fn default() -> Self {
        Self { remind_minutes: 5 }
    }
}

/// Composio's MCP server: the apps connected there (Jira, Trello, Slack,
/// Gmail, Notion and more) become tools in Ask mode. The local model may
/// only read; anything that changes something goes to Claude Code.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ComposioSettings {
    pub enabled: bool,
    /// Who signed in (their email), for Settings. The key itself is in
    /// Credential Manager.
    pub account: String,
    /// The Composio user the apps are connected under.
    pub user_id: String,
    /// The MCP server link from Composio (or from Claude Code's config).
    pub url: String,
    /// Extra request headers, such as `x-api-key`. Secrets: kept only here
    /// and left out of backups.
    pub headers: BTreeMap<String, String>,
}

/// Voice (FR-VOICE): off until the user turns it on and downloads the models.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct VoiceSettings {
    pub enabled: bool,
    /// Listen for "Hey Sidekick". Off leaves push to talk in Ask mode.
    pub wake_word: bool,
    /// Read answers aloud when the question was spoken.
    pub speak_answers: bool,
    /// After a spoken answer, listen for a reply without the wake phrase.
    pub conversation: bool,
    /// Read suggestions aloud and take a spoken choice ("open", "not now").
    pub speak_suggestions: bool,
    /// Kokoro voice id, e.g. "af_bella".
    pub voice: String,
    /// 0.5 to 2.0.
    pub speed: f32,
}

impl Default for VoiceSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            wake_word: true,
            speak_answers: true,
            conversation: true,
            speak_suggestions: true,
            voice: "af_bella".into(),
            speed: 1.0,
        }
    }
}

/// AI tiers (FR-AI-09: each can be switched off; Sidekick works without any).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AiSettings {
    /// Chat providers in the order they are tried: ids from [`AI_PROVIDERS`].
    pub order: Vec<String>,
    pub claude_code: ClaudeCodePref,
    pub local: LocalModelPref,
    pub anthropic: AnthropicPref,
    pub semif: SemIfPref,
    /// Let T1 (SemIf or the local model) rank suggestion options.
    pub decisions: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ClaudeCodePref {
    pub enabled: bool,
    /// Path to `claude`; empty means look it up on PATH.
    pub path: String,
    /// Empty means Claude Code's own default.
    pub model: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LocalModelPref {
    pub enabled: bool,
    /// OpenAI-compatible base URL (Ollama by default).
    pub base_url: String,
    /// Empty means the first model the server lists.
    pub model: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AnthropicPref {
    /// Uses `ANTHROPIC_API_KEY` from the environment; the key is never stored.
    pub enabled: bool,
    pub model: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SemIfPref {
    pub enabled: bool,
    /// Program and leading arguments, e.g. `["wsl.exe", "--", "semif-score"]`.
    pub command: Vec<String>,
    pub mode: String,
    pub backend: String,
    pub model: String,
    pub revision: String,
    /// GGUF file for the llama.cpp backend (CPU or small GPUs).
    pub gguf: String,
}

pub const AI_PROVIDERS: [&str; 3] = ["claude_code", "anthropic", "local"];

impl Default for AiSettings {
    fn default() -> Self {
        Self {
            order: AI_PROVIDERS.map(String::from).to_vec(),
            claude_code: ClaudeCodePref::default(),
            local: LocalModelPref::default(),
            anthropic: AnthropicPref::default(),
            semif: SemIfPref::default(),
            decisions: true,
        }
    }
}

impl Default for ClaudeCodePref {
    fn default() -> Self {
        Self {
            enabled: true,
            path: String::new(),
            model: String::new(),
        }
    }
}

impl Default for LocalModelPref {
    fn default() -> Self {
        Self {
            enabled: true,
            base_url: "http://localhost:11434/v1".into(),
            model: String::new(),
        }
    }
}

impl Default for AnthropicPref {
    fn default() -> Self {
        Self {
            enabled: true,
            model: String::new(),
        }
    }
}

impl Default for SemIfPref {
    fn default() -> Self {
        Self {
            enabled: false,
            command: vec!["semif-score".into()],
            mode: "direct".into(),
            backend: "llamacpp".into(),
            model: "openbmb/MiniCPM5-2B".into(),
            revision: "main".into(),
            gguf: String::new(),
        }
    }
}

impl AiSettings {
    fn sanitized(mut self) -> Self {
        let mut order: Vec<String> = Vec::new();
        for id in self
            .order
            .iter()
            .chain(AI_PROVIDERS.map(String::from).iter())
        {
            if AI_PROVIDERS.contains(&id.as_str()) && !order.contains(id) {
                order.push(id.clone());
            }
        }
        self.order = order;
        if self.local.base_url.trim().is_empty() {
            self.local.base_url = LocalModelPref::default().base_url;
        }
        self.semif.command.retain(|a| !a.trim().is_empty());
        if !["direct", "serial", "shared"].contains(&self.semif.mode.as_str()) {
            self.semif.mode = "direct".into();
        }
        if !["torch", "mlx", "llamacpp"].contains(&self.semif.backend.as_str()) {
            self.semif.backend = "llamacpp".into();
        }
        self
    }
}

/// The user's overrides for one skill. `None` keeps the skill's default.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SkillPref {
    pub enabled: Option<bool>,
    /// Run the first safe option without asking.
    pub auto: Option<bool>,
}

/// Sensors that only run when switched on explicitly.
pub const SENSORS_OFF_BY_DEFAULT: [&str; 1] = ["heartbeat"];

pub const THEMES: [&str; 3] = ["pearl", "graphite", "midnight"];
pub const SOUND_KITS: [&str; 1] = ["01"];

impl Default for Settings {
    fn default() -> Self {
        Self {
            muted: false,
            master_volume: 0.6,
            cue_volumes: BTreeMap::new(),
            collapse_after_secs: 8,
            launch_at_login: false,
            sensors: BTreeMap::new(),
            pause: Pause::None,
            theme: THEMES[0].to_string(),
            sound_kit: SOUND_KITS[0].to_string(),
            skills: BTreeMap::new(),
            palette_hotkey: DEFAULT_PALETTE_HOTKEY.into(),
            shortcuts: default_shortcuts(),
            code_folders: Vec::new(),
            end_of_day_hour: 18,
            index_folders: Vec::new(),
            ai: AiSettings::default(),
            voice: VoiceSettings::default(),
            calendar: CalendarSettings::default(),
            composio: ComposioSettings::default(),
            semantic_search: SemanticSearch::default(),
            onboarded: false,
            check_updates: true,
            deny_apps: DEFAULT_DENY_APPS.iter().map(|s| (*s).to_owned()).collect(),
            deny_sites: Vec::new(),
        }
    }
}

pub const DEFAULT_PALETTE_HOTKEY: &str = "Alt+Space";

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("settings io: {0}")]
    Io(#[from] std::io::Error),
    #[error("settings json: {0}")]
    Json(#[from] serde_json::Error),
}

impl Settings {
    /// Loads settings, falling back to defaults when the file is missing.
    /// A corrupt file is logged and replaced by defaults rather than failing
    /// app start.
    pub fn load(path: &Path) -> Self {
        match fs::read_to_string(path) {
            Ok(raw) => serde_json::from_str::<Settings>(&raw)
                .map(Settings::sanitized)
                .unwrap_or_else(|err| {
                    log::warn!(
                        "invalid settings at {}: {err}; using defaults",
                        path.display()
                    );
                    Settings::default()
                }),
            Err(_) => Settings::default(),
        }
    }

    /// Writes atomically: temp file, then rename.
    pub fn save(&self, path: &Path) -> Result<(), SettingsError> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        fs::rename(tmp, path)?;
        Ok(())
    }

    /// Clamps values that came from the UI or a hand-edited file.
    pub fn sanitized(mut self) -> Self {
        self.master_volume = self.master_volume.clamp(0.0, 1.0);
        for v in self.cue_volumes.values_mut() {
            *v = v.clamp(0.0, 1.0);
        }
        self.collapse_after_secs = self.collapse_after_secs.clamp(2, 120);
        if !THEMES.contains(&self.theme.as_str()) {
            self.theme = THEMES[0].to_string();
        }
        if !SOUND_KITS.contains(&self.sound_kit.as_str()) {
            self.sound_kit = SOUND_KITS[0].to_string();
        }
        if self.palette_hotkey.trim().is_empty() {
            self.palette_hotkey = DEFAULT_PALETTE_HOTKEY.into();
        }
        self.ai = self.ai.sanitized();
        self.voice.speed = if self.voice.speed.is_finite() {
            self.voice.speed.clamp(0.5, 2.0)
        } else {
            1.0
        };
        if self.semantic_search.model.trim().is_empty() {
            self.semantic_search.model = SemanticSearch::default().model;
        }
        self.calendar.remind_minutes = self.calendar.remind_minutes.clamp(1, 30);
        if self.voice.voice.trim().is_empty() {
            self.voice.voice = VoiceSettings::default().voice;
        }
        self.end_of_day_hour = self.end_of_day_hour.min(23);
        self.code_folders.retain(|f| !f.trim().is_empty());
        for (action, default) in SHORTCUTS {
            self.shortcuts
                .entry((*action).to_owned())
                .or_insert_with(|| (*default).to_owned());
        }
        self.shortcuts
            .retain(|k, _| SHORTCUTS.iter().any(|(a, _)| a == k));
        self.deny_apps.retain(|a| !a.trim().is_empty());
        self.deny_sites.retain(|s| !s.trim().is_empty());
        self.index_folders.retain(|f| !f.trim().is_empty());
        self
    }

    pub fn sensor_enabled(&self, id: &str) -> bool {
        self.sensors
            .get(id)
            .copied()
            .unwrap_or(!SENSORS_OFF_BY_DEFAULT.contains(&id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ai_order_keeps_known_ids_once_and_adds_missing() {
        let mut s = Settings::default();
        s.ai.order = vec!["local".into(), "bogus".into(), "local".into()];
        s.ai.semif.mode = "weird".into();
        s.palette_hotkey = " ".into();
        let s = s.sanitized();
        assert_eq!(s.ai.order, ["local", "claude_code", "anthropic"]);
        assert_eq!(s.ai.semif.mode, "direct");
        assert_eq!(s.palette_hotkey, DEFAULT_PALETTE_HOTKEY);
    }

    #[test]
    fn old_settings_files_get_ai_defaults() {
        let s: Settings = serde_json::from_str(r#"{"muted":true}"#).unwrap();
        assert!(s.muted);
        assert!(s.ai.claude_code.enabled);
        assert_eq!(s.ai.local.base_url, "http://localhost:11434/v1");
        assert!(!s.ai.semif.enabled);
    }

    #[test]
    fn pause_until_expires() {
        let now = Utc::now();
        let pause = Pause::for_minutes(15, now);
        assert!(pause.is_active(now));
        assert!(!pause.is_active(now + Duration::minutes(16)));
        assert!(Pause::Indefinite.is_active(now));
        assert!(!Pause::None.is_active(now));
    }

    #[test]
    fn pause_serializes_as_tagged_object() {
        assert_eq!(
            serde_json::to_value(Pause::None).unwrap(),
            serde_json::json!({"kind": "none"})
        );
        assert_eq!(
            serde_json::to_value(Pause::Indefinite).unwrap(),
            serde_json::json!({"kind": "indefinite"})
        );
    }

    #[test]
    fn round_trips_through_disk_and_fills_missing_fields() {
        let dir = std::env::temp_dir().join(format!("sidekick-settings-{}", ulid::Ulid::new()));
        let path = dir.join("settings.json");
        let s = Settings {
            muted: true,
            sensors: BTreeMap::from([("heartbeat".to_string(), false)]),
            ..Settings::default()
        };
        s.save(&path).unwrap();
        let loaded = Settings::load(&path);
        assert_eq!(loaded, s);
        assert!(!loaded.sensor_enabled("heartbeat"));
        assert!(loaded.sensor_enabled("files"));
        assert!(!Settings::default().sensor_enabled("heartbeat"));

        fs::write(&path, r#"{"muted": true}"#).unwrap();
        let partial = Settings::load(&path);
        assert!(partial.muted);
        assert_eq!(partial.collapse_after_secs, 8);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn unknown_theme_and_kit_fall_back_to_defaults() {
        let s = Settings {
            theme: "neon".into(),
            sound_kit: "99".into(),
            ..Settings::default()
        }
        .sanitized();
        assert_eq!(s.theme, "pearl");
        assert_eq!(s.sound_kit, "01");
    }

    #[test]
    fn sanitizes_out_of_range_values() {
        let s = Settings {
            master_volume: 3.0,
            collapse_after_secs: 0,
            ..Settings::default()
        }
        .sanitized();
        assert_eq!(s.master_volume, 1.0);
        assert_eq!(s.collapse_after_secs, 2);
    }

    #[test]
    fn corrupt_file_falls_back_to_defaults() {
        let dir = std::env::temp_dir().join(format!("sidekick-settings-{}", ulid::Ulid::new()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        fs::write(&path, "not json").unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
        fs::remove_dir_all(dir).unwrap();
    }
}
