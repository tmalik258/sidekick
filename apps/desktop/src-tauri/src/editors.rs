//! The code editor projects and files open in: the one chosen in Settings,
//! or (Auto) the one used most this week, from time tracking.

use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::state::{AppState, executor, lock};

/// Auto may change its pick as the week goes on.
const RECHECK: Duration = Duration::from_secs(30 * 60);

/// Minutes per app over the last 7 days.
fn week_minutes(app: &AppHandle) -> Vec<(String, i64)> {
    let since = (chrono::Local::now() - chrono::Duration::days(6))
        .format("%Y-%m-%d")
        .to_string();
    lock(&app.state::<AppState>().storage)
        .time_by_app_since(&since)
        .unwrap_or_default()
        .into_iter()
        .map(|(app, secs)| (app, secs / 60))
        .collect()
}

/// Applies the editor choice to the current capabilities.
pub fn refresh(app: &AppHandle) {
    let state = app.state::<AppState>();
    let choice = lock(&state.settings).code_editor.clone();
    let mut caps = executor(&state).capabilities().clone();
    let before = caps.code.clone();
    caps.use_editor(&choice, &week_minutes(app));
    if caps.code == before {
        return;
    }
    log::info!(
        "projects open in {}",
        caps.code_name.as_deref().unwrap_or("no editor")
    );
    *state
        .executor
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) =
        std::sync::Arc::new(sidekick_actions::Executor::new(caps));
}

/// Picks the editor now and again every half hour.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            refresh(&app);
            tokio::time::sleep(RECHECK).await;
        }
    });
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorInfo {
    id: String,
    name: String,
    /// Minutes used in the last 7 days.
    minutes: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Editors {
    editors: Vec<EditorInfo>,
    /// The editor projects open in now.
    current: Option<String>,
}

/// Installed editors with this week's time, for Settings.
pub fn list(app: &AppHandle) -> Editors {
    let state = app.state::<AppState>();
    let exec = executor(&state);
    let caps = exec.capabilities();
    let week = week_minutes(app);
    Editors {
        editors: caps
            .editors
            .iter()
            .map(|e| EditorInfo {
                id: e.id.clone(),
                name: e.name.clone(),
                minutes: week
                    .iter()
                    .filter(|(a, _)| e.is_app(a))
                    .map(|(_, m)| m)
                    .sum(),
            })
            .collect(),
        current: caps.code_name.clone(),
    }
}
