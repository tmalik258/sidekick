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
pub const DAY_SUMMARY: &str = "time.day_summary";
/// A summary is only worth showing after this much time at the computer.
const MIN_SUMMARY_SECS: i64 = 60 * 60;

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
    let project = if crate::routines::is_browser(exe) {
        crate::routines::current_site()
    } else {
        project_from_title(exe, title)
    };
    *current = Some(Span {
        project,
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

/// The user is at the computer with an app in front.
pub fn is_active(app: &AppHandle) -> bool {
    lock(&app.state::<AppState>().tracker.current).is_some()
}

pub fn human(secs: i64) -> String {
    let (h, m) = (secs / 3600, (secs % 3600) / 60);
    if h > 0 {
        format!("{h} h {m} min")
    } else {
        format!("{m} min")
    }
}

/// Time per project (or app when there is no project), longest first.
pub fn by_name(rows: &[sidekick_core::AppTime]) -> Vec<(String, i64)> {
    let mut by_name: Vec<(String, i64)> = Vec::new();
    for r in rows {
        let name = if r.project.is_empty() {
            r.app.clone()
        } else {
            r.project.clone()
        };
        match by_name.iter_mut().find(|(n, _)| *n == name) {
            Some((_, s)) => *s += r.secs,
            None => by_name.push((name, r.secs)),
        }
    }
    by_name.sort_by_key(|(_, s)| std::cmp::Reverse(*s));
    by_name
}

/// The day's summary event (FR-COMM-04): time per project (or app), with a
/// plain-text version ready to paste into a standup or timesheet.
pub fn day_summary(rows: &[sidekick_core::AppTime]) -> Option<Event> {
    let total: i64 = rows.iter().map(|r| r.secs).sum();
    if total < MIN_SUMMARY_SECS {
        return None;
    }
    let by_name = by_name(rows);
    let top: Vec<String> = by_name
        .iter()
        .take(3)
        .map(|(n, s)| format!("{n} {}", human(*s)))
        .collect();
    let lines: Vec<String> = by_name
        .iter()
        .filter(|(_, s)| *s >= 5 * 60)
        .map(|(n, s)| format!("- {n}: {}", human(*s)))
        .collect();
    let text = format!("Today ({} total)\n{}", human(total), lines.join("\n"));
    Some(Event::new(
        DAY_SUMMARY,
        "time",
        serde_json::json!({ "total_human": human(total), "top": top.join(", "), "text": text }),
    ))
}

pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut summarized: Option<chrono::NaiveDate> = None;
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
            // Once a day, after the end-of-day hour, offer the summary.
            let local = Local::now();
            let eod = lock(&state.settings).end_of_day_hour;
            if chrono::Timelike::hour(&local) >= eod && summarized != Some(local.date_naive()) {
                summarized = Some(local.date_naive());
                let rows = lock(&state.storage)
                    .time_for_day(&today())
                    .unwrap_or_default();
                if let Some(e) = day_summary(&rows) {
                    state.bus.publish(e);
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
    fn summarizes_the_day_by_project() {
        let row = |app: &str, project: &str, secs| sidekick_core::AppTime {
            app: app.into(),
            project: project.into(),
            secs,
        };
        let rows = [
            row("Code", "sidekick", 7200),
            row("Cursor", "sidekick", 1800),
            row("Chrome", "", 2400),
            row("Slack", "", 120),
        ];
        let e = day_summary(&rows).unwrap();
        assert_eq!(e.payload["total_human"], "3 h 12 min");
        assert_eq!(
            e.payload["top"],
            "sidekick 2 h 30 min, Chrome 40 min, Slack 2 min"
        );
        let text = e.payload["text"].as_str().unwrap();
        assert!(text.contains("- sidekick: 2 h 30 min"));
        assert!(!text.contains("Slack"), "under five minutes is left out");
        assert!(day_summary(&[row("Code", "x", 600)]).is_none());
    }

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
