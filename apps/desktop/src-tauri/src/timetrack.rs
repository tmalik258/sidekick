//! Time per app and project (FR-SYS-06), from the active-window events. It
//! is stored locally only, pauses while you are away, and is flushed to
//! SQLite every minute. A long stretch in an editor raises
//! `focus.long_session` (FR-SYS-05), which the Focus skill turns into a
//! Do Not Disturb suggestion.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use chrono::Local;
use serde_json::Value;
use sidekick_core::Event;
use tauri::{AppHandle, Manager};

use crate::state::{AppState, lock};

const FLUSH_EVERY: Duration = Duration::from_secs(60);
/// Continuous editor time before Focus mode is offered.
const FOCUS_AFTER: Duration = Duration::from_secs(20 * 60);
pub const LONG_SESSION: &str = "focus.long_session";

const EDITORS: &[&str] = &[
    "code.exe",
    "cursor.exe",
    "windsurf.exe",
    "zed.exe",
    "devenv.exe",
    "idea64.exe",
    "pycharm64.exe",
    "webstorm64.exe",
    "rider64.exe",
    "sublime_text.exe",
    "notepad++.exe",
    "code",
    "cursor",
];

#[derive(Debug, Clone)]
struct Span {
    app: String,
    project: String,
    editor: bool,
    /// Last moment time was written for this span.
    counted_to: Instant,
    /// When this stretch in an editor began (survives switching files).
    editor_since: Option<Instant>,
    focus_offered: bool,
}

#[derive(Default)]
pub struct Tracker {
    current: Mutex<Option<Span>>,
}

/// The project from a window title, for editors that show it: VS Code and
/// friends use "file - project - Visual Studio Code".
pub fn project_from_title(exe: &str, title: &str) -> String {
    if !EDITORS.contains(&exe) {
        return String::new();
    }
    let parts: Vec<&str> = title.split(" - ").map(str::trim).collect();
    match parts.len() {
        0 | 1 => String::new(),
        2 => parts[0].trim_start_matches('●').trim().to_owned(),
        n => parts[n - 2].trim_start_matches('●').trim().to_owned(),
    }
}

fn today() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}

fn write(app: &AppHandle, span: &mut Span, now: Instant) {
    let secs = now.duration_since(span.counted_to).as_secs() as i64;
    if secs <= 0 {
        return;
    }
    span.counted_to = now;
    let state = app.state::<AppState>();
    if let Err(err) = lock(&state.storage).add_time(&today(), &span.app, &span.project, secs) {
        log::warn!("could not record app time: {err}");
    }
}

/// A new app came to the front.
pub fn on_window(app: &AppHandle, payload: &Value) {
    let name = payload["app"].as_str().unwrap_or_default().to_owned();
    if name.is_empty() {
        return;
    }
    let exe = payload["exe"].as_str().unwrap_or_default();
    let title = payload["title"].as_str().unwrap_or_default();
    let now = Instant::now();
    let state = app.state::<AppState>();
    let mut current = lock(&state.tracker.current);
    let carried_editor = current.as_ref().and_then(|s| s.editor_since);
    let carried_offer = current.as_ref().is_some_and(|s| s.focus_offered);
    if let Some(span) = current.as_mut() {
        write(app, span, now);
    }
    let editor = EDITORS.contains(&exe);
    *current = Some(Span {
        project: project_from_title(exe, title),
        app: name,
        editor,
        counted_to: now,
        editor_since: if editor {
            carried_editor.or(Some(now))
        } else {
            None
        },
        focus_offered: editor && carried_offer,
    });
}

/// The user stepped away: stop counting until they are back.
pub fn on_away(app: &AppHandle) {
    let state = app.state::<AppState>();
    let mut current = lock(&state.tracker.current);
    if let Some(span) = current.as_mut() {
        write(app, span, Instant::now());
    }
    *current = None;
}

/// Writes the running span every minute and raises a long editor session.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(FLUSH_EVERY).await;
            let now = Instant::now();
            let state = app.state::<AppState>();
            let mut long_session = None;
            {
                let mut current = lock(&state.tracker.current);
                if let Some(span) = current.as_mut() {
                    write(&app, span, now);
                    if span.editor
                        && !span.focus_offered
                        && span
                            .editor_since
                            .is_some_and(|t| now.duration_since(t) >= FOCUS_AFTER)
                    {
                        span.focus_offered = true;
                        long_session = Some((span.app.clone(), span.project.clone()));
                    }
                }
            }
            if let Some((name, project)) = long_session {
                let minutes = FOCUS_AFTER.as_secs() / 60;
                state.bus.publish(Event::new(
                    LONG_SESSION,
                    "time",
                    serde_json::json!({ "app": name, "project": project, "minutes": minutes }),
                ));
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_projects_from_editor_titles() {
        assert_eq!(
            project_from_title("code.exe", "Island.tsx - sidekick - Visual Studio Code"),
            "sidekick"
        );
        assert_eq!(
            project_from_title("code.exe", "● main.py - api - Visual Studio Code"),
            "api"
        );
        assert_eq!(
            project_from_title("code.exe", "sidekick - Visual Studio Code"),
            "sidekick"
        );
        assert_eq!(project_from_title("chrome.exe", "Docs - Google Chrome"), "");
        assert_eq!(project_from_title("code.exe", "Visual Studio Code"), "");
    }
}
