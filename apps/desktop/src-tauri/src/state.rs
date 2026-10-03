use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};
use std::time::Instant;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sidekick_actions::{Capabilities, Executor};
use sidekick_core::{EventBus, MascotMachine, Settings, Storage};
use sidekick_sensors::{GateState, SensorGateHandle};
use sidekick_skills::{Engine, Env, Proposal, Skill, Trust};

/// Every sensor the app can run, in the order shown in settings.
pub const SENSOR_IDS: [&str; 11] = [
    "calendar",
    "downloads",
    "screenshots",
    "ports",
    "clipboard",
    "window",
    "claude_code",
    "browser",
    "system",
    "repos",
    "idle",
];

pub struct AppState {
    pub settings: Mutex<Settings>,
    pub settings_path: PathBuf,
    pub storage: Arc<Mutex<Storage>>,
    pub db_path: PathBuf,
    pub skills_dir: PathBuf,
    pub bus: EventBus,
    pub gate: SensorGateHandle,
    pub mascot: Mutex<MascotMachine>,
    /// Bumped on every mascot transition so delayed follow-ups can tell
    /// whether the state they were scheduled for is still current.
    pub mascot_epoch: AtomicU64,
    pub hit_rect: Mutex<HitRect>,
    pub engine: Mutex<Engine>,
    pub skill_errors: Mutex<Vec<String>>,
    pub executor: RwLock<Arc<Executor>>,
    /// The suggestion on screen, if any.
    pub active: Mutex<Option<Active>>,
    /// Suggestions waiting for the island to be free.
    pub queue: Mutex<VecDeque<Queued>>,
    /// Suggestions waiting quietly; the island shows how many.
    pub later: Mutex<Vec<Later>>,
    pub island_hidden: Mutex<bool>,
    /// The cursor is over the island (kept by the hover tracker).
    pub hovered: AtomicBool,
    /// The last app the user was in (payload of `window.focused`).
    pub last_window: Mutex<Option<serde_json::Value>>,
    /// Chats in flight, so they can be cancelled.
    pub chats: Mutex<HashMap<String, sidekick_ai::CancellationToken>>,
    /// Actions Ask offered as buttons, waiting for a tap.
    pub ask_proposals: Mutex<HashMap<String, crate::ask_tools::Proposed>>,
    /// Empty folder Claude Code runs in, so it has no project to touch.
    pub ai_workdir: PathBuf,
    /// Temporary files (SemIf input and output).
    pub scratch_dir: PathBuf,
    /// Some AI provider is switched on and reachable (skills `requires: [ai]`).
    pub ai_ready: AtomicBool,
    /// Files Sidekick's own actions just created, so the downloads sensor
    /// seeing them does not trigger a suggestion about Sidekick's output.
    pub own_files: Mutex<HashMap<PathBuf, Instant>>,
    /// Time per app and project.
    pub tracker: crate::timetrack::Tracker,
    /// Commands for the browser extension.
    pub browser: sidekick_sensors::BrowserBridge,
    /// The pairing code the extension must send.
    pub browser_token: String,
    /// Bearer token for the MCP server.
    pub mcp_token: String,
    /// The island is in Ask mode (input, commands, chat).
    pub ask_open: AtomicBool,
    /// The next success has buttons (Undo, Show in folder); hold it longer.
    pub linger: AtomicBool,
    /// The user stepped away (no input for a while); suggestions wait.
    pub away: AtomicBool,
    /// T1 picks per skill and app, from earlier decisions.
    pub decisions: Mutex<crate::decide::Cache>,
    pub voice: crate::voice::Voice,
    pub calendar: sidekick_sensors::Calendar,
    /// Claude Code permission requests waiting on the island (FR-DEV-06).
    pub approvals: sidekick_sensors::Approvals,
    /// Submitted login waiting for auto-save / edit (password only in memory).
    pub pending_password: Mutex<crate::password_save::PasswordBook>,
}

/// The interactive part of the island window, in logical pixels relative to
/// the window. Everything outside it is click-through.
#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HitRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl HitRect {
    pub fn contains(&self, x: f64, y: f64) -> bool {
        self.contains_within(x, y, 0.0)
    }

    /// Like [`HitRect::contains`], with `pad` pixels to spare on every side.
    pub fn contains_within(&self, x: f64, y: f64, pad: f64) -> bool {
        x >= self.x - pad
            && x <= self.x + self.width + pad
            && y >= self.y - pad
            && y <= self.y + self.height + pad
    }
}

/// A suggestion as the UI sees it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    pub id: String,
    pub skill_id: String,
    pub title: String,
    pub detail: String,
    pub options: Vec<String>,
    /// Which options can become "Always do this": safe to run on their own.
    pub always: Vec<bool>,
}

pub struct Active {
    pub ui: Suggestion,
    pub proposal: Proposal,
}

pub struct Queued {
    pub proposal: Proposal,
    pub at: Instant,
}

/// A suggestion kept for later instead of interrupting: a minor one, one
/// that came in during a meeting, or one shown that nobody acted on.
pub struct Later {
    pub id: String,
    pub proposal: Proposal,
    pub at: chrono::DateTime<chrono::Utc>,
    /// It was shown and timed out, rather than held back.
    pub missed: bool,
}

pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

pub fn executor(state: &AppState) -> Arc<Executor> {
    state
        .executor
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

/// Whether the user paused Sidekick: nothing shows or runs on its own.
pub fn is_paused(app: &tauri::AppHandle) -> bool {
    use tauri::Manager;
    lock(&app.state::<AppState>().settings)
        .pause
        .is_active(Utc::now())
}

pub fn gate_state(settings: &Settings, now: DateTime<Utc>) -> GateState {
    GateState {
        paused: settings.pause.is_active(now),
        disabled: SENSOR_IDS
            .iter()
            .filter(|id| !settings.sensor_enabled(id))
            .map(|id| id.to_string())
            .collect(),
    }
}

/// What the skill engine needs from the app, captured for one evaluation.
pub struct AppEnv<'a> {
    pub settings: &'a Settings,
    pub ai_ready: bool,
    pub caps: &'a Capabilities,
    pub storage: &'a Mutex<Storage>,
}

impl Env for AppEnv<'_> {
    fn has(&self, requirement: &str) -> bool {
        match requirement {
            "ai" => self.ai_ready,
            "tool:browser_passwords" => self.caps.browsers.iter().any(|b| {
                self.settings
                    .password_browsers
                    .as_ref()
                    .is_none_or(|ids| ids.contains(&b.id))
                    && sidekick_actions::passwords::user_data_dir(&b.id).is_some()
            }),
            r if r.starts_with("app:") => crate::composio::app_connected(&r[4..]),
            _ => self.caps.has(requirement),
        }
    }

    fn skill_enabled(&self, skill: &Skill) -> bool {
        self.settings
            .skills
            .get(&skill.id)
            .and_then(|p| p.enabled)
            .unwrap_or(skill.enabled_by_default)
    }

    fn trust_override(&self, skill_id: &str) -> Option<Trust> {
        self.settings
            .skills
            .get(skill_id)
            .and_then(|p| p.auto)
            .map(|auto| if auto { Trust::Auto } else { Trust::Suggest })
    }

    fn choice_counts(&self, key: &str) -> HashMap<String, u32> {
        lock(self.storage).choice_counts(key).unwrap_or_default()
    }

    fn default_browser_id(&self) -> Option<String> {
        sidekick_actions::default_browser()
            .filter(|id| self.caps.browser(id).is_some())
            .map(|id| id.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sidekick_core::Pause;

    #[test]
    fn disabled_password_targets_do_not_offer_fill() {
        let storage = Mutex::new(Storage::open_in_memory().unwrap());
        let caps = Capabilities {
            browsers: vec![sidekick_actions::Browser {
                id: "chrome".into(),
                path: "fixture".into(),
            }],
            ..Capabilities::default()
        };
        let settings = Settings {
            password_browsers: Some(vec![]),
            ..Settings::default()
        };
        let env = AppEnv {
            settings: &settings,
            caps: &caps,
            ai_ready: false,
            storage: &storage,
        };
        assert!(!env.has("tool:browser_passwords"));
    }

    #[test]
    fn hit_rect_contains_edges() {
        let r = HitRect {
            x: 10.0,
            y: 0.0,
            width: 100.0,
            height: 40.0,
        };
        assert!(r.contains(10.0, 0.0));
        assert!(r.contains(110.0, 40.0));
        assert!(!r.contains(9.9, 10.0));
        assert!(!r.contains(50.0, 40.1));
    }

    #[test]
    fn gate_state_reflects_pause_and_disabled_sensors() {
        let now = Utc::now();
        let mut s = Settings::default();
        s.sensors.insert("clipboard".into(), false);
        s.sensors.insert("downloads".into(), true);
        s.pause = Pause::for_minutes(5, now);
        let g = gate_state(&s, now);
        assert!(g.paused);
        // clipboard was switched off; everything else defaults on.
        assert_eq!(g.disabled.into_iter().collect::<Vec<_>>(), ["clipboard"]);
    }
}
