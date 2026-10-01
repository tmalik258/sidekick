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
pub const SENSOR_IDS: [&str; 8] = [
    "downloads",
    "ports",
    "clipboard",
    "window",
    "claude_code",
    "system",
    "idle",
    "heartbeat",
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
    pub island_hidden: Mutex<bool>,
    /// The cursor is over the island (kept by the hover tracker).
    pub hovered: AtomicBool,
    /// The last app the user was in (payload of `window.focused`).
    pub last_window: Mutex<Option<serde_json::Value>>,
    /// Chats in flight, so they can be cancelled.
    pub chats: Mutex<HashMap<String, sidekick_ai::CancellationToken>>,
    /// Empty folder Claude Code runs in, so it has no project to touch.
    pub ai_workdir: PathBuf,
    /// Temporary files (SemIf input and output).
    pub scratch_dir: PathBuf,
    /// Some AI provider is switched on and reachable (skills `requires: [ai]`).
    pub ai_ready: AtomicBool,
    /// Files Sidekick's own actions just created, so the downloads sensor
    /// seeing them does not trigger a suggestion about Sidekick's output.
    pub own_files: Mutex<HashMap<PathBuf, Instant>>,
    /// The next success has buttons (Undo, Show in folder); hold it longer.
    pub linger: AtomicBool,
    /// The user stepped away (no input for a while); suggestions wait.
    pub away: AtomicBool,
    /// T1 picks per skill and app, from earlier decisions.
    pub decisions: Mutex<crate::decide::Cache>,
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
        x >= self.x && x <= self.x + self.width && y >= self.y && y <= self.y + self.height
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
}

pub struct Active {
    pub ui: Suggestion,
    pub proposal: Proposal,
}

pub struct Queued {
    pub proposal: Proposal,
    pub at: Instant,
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use sidekick_core::Pause;

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
        // heartbeat is off by default, clipboard was switched off.
        assert_eq!(
            g.disabled.into_iter().collect::<Vec<_>>(),
            ["clipboard", "heartbeat"]
        );
    }
}
