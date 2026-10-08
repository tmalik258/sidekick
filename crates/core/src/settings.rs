use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

/// Whether sensors are paused.
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
    ("focus", "Ctrl+Alt+F"),
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
    /// The island's colour behind its content: one of [`ISLAND_COLORS`].
    pub island_color: String,
    /// UI sound kit: one of [`SOUND_KITS`].
    pub sound_kit: String,
    /// Per-skill switches set by the user.
    pub skills: BTreeMap<String, SkillPref>,
    /// Global shortcut that turns the island into Ask mode.
    pub palette_hotkey: String,
    /// More global shortcuts, by action (talk, accept, dismiss, screen,
    /// clipboard, pause, settings). Empty turns one off.
    pub shortcuts: BTreeMap<String, String>,
    /// Where projects and files open: an editor id ("cursor", "pycharm"),
    /// or "auto" for the one used most this week.
    pub code_editor: String,
    /// Folders with git repos to check at the end of the day.
    /// Empty means the usual places (code, projects, source/repos, ...).
    pub code_folders: Vec<String>,
    /// Local hour after which unsaved work is reported.
    pub end_of_day_hour: u32,
    /// Where clones go, by owner ("abdullahalhoothy" to "D:\\Abdullah"),
    /// learned from where you cloned before or set in Settings.
    pub clone_rules: BTreeMap<String, String>,
    /// Folders whose text files are searchable (opt in).
    pub index_folders: Vec<String>,
    pub ai: AiSettings,
    pub voice: VoiceSettings,
    pub calendar: CalendarSettings,
    /// Composio's MCP server, for apps like Jira, Slack and Gmail in chat.
    pub composio: ComposioSettings,
    pub semantic_search: SemanticSearch,
    /// The first-run welcome was finished or skipped.
    pub onboarded: bool,
    /// Step index inside the welcome flow (0-based). Kept so a restart
    /// resumes where the user left off until they skip or finish.
    pub welcome_step: u32,
    /// Look for a newer release once a day.
    pub check_updates: bool,
    /// Programs whose windows and copies Sidekick ignores.
    pub deny_apps: Vec<String>,
    /// Sites (and their subdomains) Sidekick ignores.
    pub deny_sites: Vec<String>,
    /// Learn what you open first each day and offer it as one card.
    pub routines: bool,
    /// Open the usual setup without asking (offered after five Open alls).
    pub routines_auto: bool,
    /// Fade the island out while a fullscreen app is in front. Off: the
    /// island stays on top of everything, fullscreen apps included.
    pub hide_in_fullscreen: bool,
    /// Now and then, when nothing needs you, a short tip on the island.
    pub tips: bool,
    /// The mascot idles on its own: glances around, blinks, the odd smile.
    pub alive: bool,
    /// After a crash, offer a report to send (never sent on its own).
    pub crash_reports: bool,
    /// The notification inbox: Sidekick reads Windows notifications and only
    /// brings up what matters.
    pub notifications: NotificationSettings,
    /// Saved tasks that run again on a trigger (or on request).
    pub recipes: Vec<Recipe>,
    /// Things Sidekick knows about the user ("My manager is Sara"), given to
    /// every model. Edited in Settings or learned when the user says so.
    pub memory: Vec<String>,
    /// What Sidekick calls the user ("Sam"); empty until they say.
    pub user_name: String,
    /// People the user works with ("Sara, my manager"), one per line.
    pub people: String,
    /// What the user is working on now, one per line.
    pub projects: String,
    /// How the user likes answers ("short, bullet points").
    pub answer_style: String,
    /// The user works with code: technical answers and setup. None: not asked.
    pub codes: Option<bool>,
    /// What the assistant is called and answers to ("Hey Orbi"). The app
    /// itself stays Sidekick.
    pub assistant_name: String,
    /// Learn from choices (links, files, suggestions, routines). Off: nothing
    /// new is learned; what is known stays until forgotten.
    pub learning: bool,
    /// How much Sidekick asks before acting in apps and pages.
    pub agent: AgentSettings,
}

/// When an agent step waits for the user's tap.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AgentSettings {
    /// "each": every step; "outward": anything that sends, posts, pays or
    /// deletes (the default); "irreversible": only paying and deleting.
    pub ask: String,
    /// Per app or site ("whatsapp", "mail.google.com"): "allow", "ask" or
    /// "never". Allow skips the tap except for paying and deleting; never
    /// keeps Sidekick out entirely.
    pub places: BTreeMap<String, String>,
}

impl Default for AgentSettings {
    fn default() -> Self {
        Self {
            ask: "outward".into(),
            places: BTreeMap::new(),
        }
    }
}

/// A saved task: what to do (in the user's words) and when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Recipe {
    pub id: String,
    pub name: String,
    /// The instruction, as it would be typed in Ask.
    pub prompt: String,
    pub trigger: Trigger,
    /// Start without asking first. Sending, posting and paying still wait
    /// for a tap.
    pub auto: bool,
    pub enabled: bool,
}

impl Default for Recipe {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            prompt: String::new(),
            trigger: Trigger::default(),
            auto: false,
            enabled: true,
        }
    }
}

/// When a recipe runs.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "when", rename_all = "snake_case")]
pub enum Trigger {
    /// Only when asked (by name in Ask, or Run in Settings).
    #[default]
    Manual,
    /// At a local time ("17:00") on some days ("mon".."sun"; empty is daily).
    Time { time: String, days: Vec<String> },
    /// A notification from an app, optionally only when its text has a word.
    Notification { app: String, contains: String },
    /// A download finished, optionally only of one kind (pdf, image...).
    Download { kind: String },
    /// A calendar meeting ended.
    MeetingEnded,
    /// An app came to the front (once a day at most).
    AppOpened { app: String },
}

/// How much a notification interrupts.
pub const NOTIFY_LEVELS: &[&str] = &["now", "soon", "digest", "never"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NotificationSettings {
    /// Read and sort Windows notifications.
    pub enabled: bool,
    /// The user's level per app ("WhatsApp" -> "soon"), from [`NOTIFY_LEVELS`].
    /// Missing means Sidekick decides.
    pub apps: BTreeMap<String, String>,
    /// People whose messages always come through right away.
    pub vip: Vec<String>,
}

impl Default for NotificationSettings {
    /// On from the start: nothing changes until Windows is silenced, and
    /// then only what matters comes up.
    fn default() -> Self {
        Self {
            enabled: true,
            apps: BTreeMap::new(),
            vip: Vec::new(),
        }
    }
}

/// Password managers are ignored from the start.
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

/// Search by meaning with an embedding model on this PC.
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

/// Meeting reminders; meetings come from Composio.
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

/// Voice: on by default. Speech models download on launch when
/// missing. The microphone is open only while voice is on and Sidekick is not
/// paused.
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
    /// Talking over a spoken answer stops it and listens.
    pub interrupt: bool,
    /// While you talk: "compact" (one slim line) or "full" (a waveform and
    /// your words larger).
    pub listening_style: String,
    /// Voice id, e.g. "f5" (Supertonic 3's Female 5).
    pub voice: String,
    /// 0.5 to 2.0.
    pub speed: f32,
    /// Which voice model the voice was picked for (1 Kokoro v0.19, 2 Kokoro
    /// v1.0, 3 Supertonic 3). Files saved before had no such field and read
    /// as 1. A voice from an older model is moved to the default once.
    #[serde(default = "voice_model_v1")]
    pub model: u32,
    /// Words voice gets wrong, comma separated: "horsepot = hotspot".
    pub fixes: String,
}

/// Settings saved before the v1.0 voice model had no `model` field.
fn voice_model_v1() -> u32 {
    1
}

pub const VOICE_MODEL: u32 = 3;

impl Default for VoiceSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            wake_word: true,
            speak_answers: true,
            listening_style: "compact".into(),
            conversation: true,
            speak_suggestions: true,
            interrupt: true,
            voice: "f5".into(),
            speed: 1.0,
            model: VOICE_MODEL,
            fixes: String::new(),
        }
    }
}

/// AI providers (each can be switched off; Sidekick works without any).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AiSettings {
    /// Chat providers in the order they are tried: ids from [`AI_PROVIDERS`].
    pub order: Vec<String>,
    pub claude_code: ClaudeCodePref,
    pub codex: CodexPref,
    /// Which coding agent gets handoffs and changes: "auto" (Claude Code
    /// when installed, else Codex), "claude_code" or "codex".
    pub coding_agent: String,
    pub local: LocalModelPref,
    pub anthropic: AnthropicPref,
    /// Free cloud models: Gemini first, Groq when Gemini is busy.
    pub gemini: CloudPref,
    pub groq: CloudPref,
    /// One key for hundreds of models, some free.
    pub openrouter: CloudPref,
    pub semif: SemIfPref,
    /// Let SemIf or the local model rank suggestion options.
    pub decisions: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ClaudeCodePref {
    pub enabled: bool,
    /// Path to `claude`; empty means look it up on PATH.
    pub path: String,
    /// Explicit model; legacy empty selections use the fast Haiku model.
    pub model: String,
}

/// OpenAI's Codex CLI, the same way as Claude Code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CodexPref {
    pub enabled: bool,
    /// Path to `codex`; empty means look it up on PATH.
    pub path: String,
    /// Explicit model; legacy empty selections use the fast Luna model.
    pub model: String,
}

impl Default for CodexPref {
    fn default() -> Self {
        Self {
            enabled: true,
            path: String::new(),
            model: FAST_CODEX_MODEL.into(),
        }
    }
}

/// Explicit fast models, matching the versioned choices shown in settings.
pub const FAST_CODEX_MODEL: &str = "gpt-6-luna";
pub const FAST_CLAUDE_MODEL: &str = "claude-haiku-4-5-20251001";

pub const CODING_AGENTS: [&str; 3] = ["auto", "claude_code", "codex"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LocalModelPref {
    pub enabled: bool,
    /// OpenAI-compatible base URL (Ollama by default).
    pub base_url: String,
    /// Empty means the first model the server lists.
    pub model: String,
    /// A small model that sees pictures (e.g. moondream), used only for
    /// questions about the screen. Empty: the screen is read as text (OCR).
    pub vision_model: String,
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

pub const AI_PROVIDERS: [&str; 7] = [
    "local",
    "gemini",
    "groq",
    "claude_code",
    "codex",
    "anthropic",
    "openrouter",
];

/// A cloud model reached with an API key kept in Credential Manager. On
/// once a key is saved; `model` empty means the provider's default.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CloudPref {
    pub enabled: bool,
    pub model: String,
}

impl Default for AiSettings {
    fn default() -> Self {
        Self {
            order: AI_PROVIDERS.map(String::from).to_vec(),
            claude_code: ClaudeCodePref::default(),
            codex: CodexPref::default(),
            coding_agent: "auto".into(),
            local: LocalModelPref::default(),
            anthropic: AnthropicPref::default(),
            gemini: CloudPref::default(),
            groq: CloudPref::default(),
            openrouter: CloudPref::default(),
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
            model: FAST_CLAUDE_MODEL.into(),
        }
    }
}

impl Default for LocalModelPref {
    fn default() -> Self {
        Self {
            enabled: true,
            base_url: "http://127.0.0.1:11434/v1".into(),
            model: String::new(),
            vision_model: String::new(),
        }
    }
}

impl Default for AnthropicPref {
    fn default() -> Self {
        Self {
            enabled: true,
            model: FAST_CLAUDE_MODEL.into(),
        }
    }
}

impl Default for SemIfPref {
    fn default() -> Self {
        Self {
            enabled: true,
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
        let had_codex = self.order.iter().any(|id| id == "codex");
        for id in self
            .order
            .iter()
            .chain(AI_PROVIDERS.map(String::from).iter())
        {
            if AI_PROVIDERS.contains(&id.as_str()) && !order.contains(id) {
                order.push(id.clone());
            }
        }
        // Settings from before Codex: it goes right after Claude Code.
        if !had_codex
            && let (Some(c), Some(x)) = (
                order.iter().position(|i| i == "claude_code"),
                order.iter().position(|i| i == "codex"),
            )
        {
            let codex = order.remove(x);
            order.insert(c + 1, codex);
        }
        self.order = order;
        for (model, fallback) in [
            (&mut self.codex.model, FAST_CODEX_MODEL),
            (&mut self.claude_code.model, FAST_CLAUDE_MODEL),
            (&mut self.anthropic.model, FAST_CLAUDE_MODEL),
        ] {
            let selected = model.trim();
            *model = if selected.is_empty() || selected == "default" {
                fallback.to_owned()
            } else {
                selected.to_owned()
            };
        }
        if !CODING_AGENTS.contains(&self.coding_agent.as_str()) {
            self.coding_agent = "auto".into();
        }
        // The old default; Ollama listens on IPv4 only (see `loopback`).
        let old = self.local.base_url.trim().trim_end_matches('/');
        if old.is_empty() || old == "http://localhost:11434/v1" {
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
/// Sensors that start switched off. Empty: every sensor is on unless the user
/// turns it off.
pub const SENSORS_OFF_BY_DEFAULT: [&str; 0] = [];

pub const THEMES: [&str; 7] = [
    "pearl", "aurora", "chrome", "peach", "mint", "lilac", "onyx",
];
/// Island colours, the default first; the glass ones are a deep tint with
/// a light rim.
pub const ISLAND_COLORS: [&str; 6] = [
    "solid_black",
    "black_glass",
    "graphite",
    "midnight",
    "smoke",
    "warm_graphite",
];
/// "sidekick" is synthesized in the app (soft tones with character);
/// "01" is the SND kit.
pub const SOUND_KITS: [&str; 2] = ["sidekick", "01"];

impl Default for Settings {
    fn default() -> Self {
        Self {
            muted: false,
            master_volume: 0.6,
            cue_volumes: BTreeMap::new(),
            collapse_after_secs: 6,
            launch_at_login: true,
            sensors: BTreeMap::new(),
            pause: Pause::None,
            theme: THEMES[0].to_string(),
            island_color: ISLAND_COLORS[0].to_string(),
            sound_kit: SOUND_KITS[0].to_string(),
            skills: BTreeMap::new(),
            palette_hotkey: DEFAULT_PALETTE_HOTKEY.into(),
            shortcuts: default_shortcuts(),
            code_editor: "auto".into(),
            code_folders: Vec::new(),
            clone_rules: BTreeMap::new(),
            end_of_day_hour: 18,
            index_folders: Vec::new(),
            ai: AiSettings::default(),
            voice: VoiceSettings::default(),
            calendar: CalendarSettings::default(),
            composio: ComposioSettings::default(),
            semantic_search: SemanticSearch::default(),
            onboarded: false,
            welcome_step: 0,
            check_updates: true,
            deny_apps: DEFAULT_DENY_APPS.iter().map(|s| (*s).to_owned()).collect(),
            deny_sites: Vec::new(),
            routines: true,
            routines_auto: false,
            hide_in_fullscreen: false,
            tips: true,
            alive: true,
            crash_reports: false,
            notifications: NotificationSettings::default(),
            recipes: Vec::new(),
            memory: Vec::new(),
            user_name: String::new(),
            people: String::new(),
            projects: String::new(),
            answer_style: String::new(),
            codes: None,
            assistant_name: "Sidekick".into(),
            learning: true,
            agent: AgentSettings::default(),
        }
    }
}

pub const DEFAULT_PALETTE_HOTKEY: &str = "Ctrl+Space";
/// Screens in the first-run welcome (Welcome, Your AI, Connect, Tools, Extras).
pub const WELCOME_STEP_COUNT: u32 = 5;

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
        // Set up before "Do you work with code?" was asked: keep everything
        // they had, Agents included.
        if self.onboarded && self.codes.is_none() {
            self.codes = Some(true);
        }
        self.master_volume = self.master_volume.clamp(0.0, 1.0);
        for v in self.cue_volumes.values_mut() {
            *v = v.clamp(0.0, 1.0);
        }
        self.collapse_after_secs = self.collapse_after_secs.clamp(2, 120);
        let name = self.assistant_name.trim();
        self.assistant_name = if name.is_empty() {
            "Sidekick".into()
        } else {
            name.chars().take(24).collect()
        };
        self.user_name = self.user_name.trim().chars().take(40).collect();
        for about in [&mut self.people, &mut self.projects, &mut self.answer_style] {
            *about = about.trim().chars().take(1000).collect();
        }
        // The dark orbs of 0.1 became Onyx.
        if matches!(self.theme.as_str(), "graphite" | "midnight") {
            self.theme = "onyx".into();
        }
        if !THEMES.contains(&self.theme.as_str()) {
            self.theme = THEMES[0].to_string();
        }
        if self.code_editor.trim().is_empty() {
            self.code_editor = "auto".into();
        }
        if !ISLAND_COLORS.contains(&self.island_color.as_str()) {
            self.island_color = ISLAND_COLORS[0].to_string();
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
        // Voices of an older model do not exist in the new one: they move
        // to the default once. A voice picked on purpose since is kept.
        if self.voice.model < VOICE_MODEL {
            self.voice.voice = VoiceSettings::default().voice;
            self.voice.model = VOICE_MODEL;
        }
        if self.voice.voice.trim().is_empty() {
            self.voice.voice = VoiceSettings::default().voice;
        }
        if self.welcome_step >= WELCOME_STEP_COUNT {
            self.welcome_step = WELCOME_STEP_COUNT.saturating_sub(1);
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
    fn people_set_up_before_the_code_question_keep_agents() {
        let old = Settings {
            onboarded: true,
            codes: None,
            ..Settings::default()
        }
        .sanitized();
        assert_eq!(old.codes, Some(true));
        let new = Settings {
            onboarded: false,
            codes: None,
            ..Settings::default()
        }
        .sanitized();
        assert_eq!(new.codes, None);
        let no = Settings {
            onboarded: true,
            codes: Some(false),
            ..Settings::default()
        }
        .sanitized();
        assert_eq!(no.codes, Some(false));
    }

    #[test]
    fn moves_older_voices_to_the_new_default_once() {
        let old: Settings = serde_json::from_str(r#"{"voice": {"voice": "af_bella"}}"#).unwrap();
        let mut old = old.sanitized();
        assert_eq!(old.voice.voice, "f5");
        let kokoro: Settings =
            serde_json::from_str(r#"{"voice": {"voice": "bm_george", "model": 2}}"#).unwrap();
        assert_eq!(kokoro.sanitized().voice.voice, "f5");
        // Picking another voice later is respected.
        old.voice.voice = "m2".into();
        assert_eq!(old.sanitized().voice.voice, "m2");
    }

    #[test]
    fn ai_order_keeps_known_ids_once_and_adds_missing() {
        let mut s = Settings::default();
        s.ai.order = vec!["local".into(), "bogus".into(), "local".into()];
        s.ai.semif.mode = "weird".into();
        s.palette_hotkey = " ".into();
        let s = s.sanitized();
        assert_eq!(
            s.ai.order,
            [
                "local",
                "gemini",
                "groq",
                "claude_code",
                "codex",
                "anthropic",
                "openrouter"
            ]
        );
        assert_eq!(s.ai.coding_agent, "auto");
        assert_eq!(s.ai.semif.mode, "direct");
        assert_eq!(s.palette_hotkey, DEFAULT_PALETTE_HOTKEY);
    }

    #[test]
    fn legacy_ai_defaults_become_explicit_fast_models_and_keep_saved_choices() {
        let legacy: Settings = serde_json::from_str(r#"{"ai":{"codex":{"model":""},"claudeCode":{"model":"default"},"anthropic":{"model":"  "}}}"#).unwrap();
        let migrated = legacy.sanitized();
        assert_eq!(migrated.ai.codex.model, FAST_CODEX_MODEL);
        assert_eq!(migrated.ai.claude_code.model, FAST_CLAUDE_MODEL);
        assert_eq!(migrated.ai.anthropic.model, FAST_CLAUDE_MODEL);
        let persisted = serde_json::to_string(&migrated).unwrap();
        let restored: Settings = serde_json::from_str(&persisted).unwrap();
        assert_eq!(restored.sanitized().ai, migrated.ai);
        let custom: Settings = serde_json::from_str(r#"{"ai":{"codex":{"model":"gpt-6-astra"},"claudeCode":{"model":"sonnet"},"anthropic":{"model":"claude-opus-5-5"}}}"#).unwrap();
        let custom = custom.sanitized();
        assert_eq!(custom.ai.codex.model, "gpt-6-astra");
        assert_eq!(custom.ai.claude_code.model, "sonnet");
        assert_eq!(custom.ai.anthropic.model, "claude-opus-5-5");
        let defaults = AiSettings::default();
        assert_eq!(defaults.codex.model, FAST_CODEX_MODEL);
        assert_eq!(defaults.claude_code.model, FAST_CLAUDE_MODEL);
        assert_eq!(defaults.anthropic.model, FAST_CLAUDE_MODEL);
    }

    #[test]
    fn old_settings_files_get_ai_defaults() {
        let s: Settings = serde_json::from_str(r#"{"muted":true}"#).unwrap();
        assert!(s.muted);
        assert!(s.ai.claude_code.enabled);
        assert_eq!(s.ai.local.base_url, "http://127.0.0.1:11434/v1");
        assert!(s.ai.semif.enabled);
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
            sensors: BTreeMap::from([("clipboard".to_string(), false)]),
            ..Settings::default()
        };
        s.save(&path).unwrap();
        let loaded = Settings::load(&path);
        assert_eq!(loaded, s);
        assert!(!loaded.sensor_enabled("clipboard"));
        assert!(loaded.sensor_enabled("files"));
        assert!(Settings::default().sensor_enabled("clipboard"));

        fs::write(&path, r#"{"muted": true}"#).unwrap();
        let partial = Settings::load(&path);
        assert!(partial.muted);
        assert_eq!(partial.collapse_after_secs, 6);
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
        assert_eq!(s.sound_kit, "sidekick");
    }

    #[test]
    fn old_dark_orbs_become_onyx_and_islands_default_to_solid_black() {
        let s = Settings {
            theme: "midnight".into(),
            island_color: "neon".into(),
            ..Settings::default()
        }
        .sanitized();
        assert_eq!(s.theme, "onyx");
        assert_eq!(s.island_color, "solid_black");
        assert_eq!(Settings::default().island_color, "solid_black");
    }

    #[test]
    fn sanitizes_out_of_range_values() {
        let s = Settings {
            master_volume: 3.0,
            collapse_after_secs: 0,
            welcome_step: 99,
            ..Settings::default()
        }
        .sanitized();
        assert_eq!(s.master_volume, 1.0);
        assert_eq!(s.collapse_after_secs, 2);
        assert_eq!(s.welcome_step, WELCOME_STEP_COUNT - 1);
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
