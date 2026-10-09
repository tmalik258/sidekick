//! Time per app and project, from the active-window events. It
//! is stored locally only, pauses while you are away, and is flushed to
//! SQLite every minute. A long stretch in an editor raises
//! `focus.long_session`, which the Focus skill turns into a
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
    let name = match parts.len() {
        0 | 1 => return String::new(),
        2 => parts[0],
        n => parts[n - 2],
    };
    // "sidekick [WSL: Ubuntu]" or "sidekick (Workspace)" is still "sidekick".
    let name = name.trim_start_matches('●').trim();
    name.split([' ', '\t'])
        .take_while(|w| !w.starts_with('[') && !w.starts_with('('))
        .collect::<Vec<_>>()
        .join(" ")
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
/// The app in front now ("Code"), if any.
pub fn current_app(app: &AppHandle) -> Option<String> {
    lock(&app.state::<AppState>().tracker.current)
        .as_ref()
        .map(|s| s.app.clone())
}

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

/// What got done today besides time: commits per project and meetings.
#[derive(Default)]
pub struct Done {
    /// (project folder name, commit subjects).
    pub commits: Vec<(String, Vec<String>)>,
    pub meetings: Vec<String>,
}

impl Done {
    /// ". Fix tool calls, Add eval (3 commits)" for a project, or "".
    fn about(&self, project: &str) -> String {
        let Some((_, subjects)) = self
            .commits
            .iter()
            .find(|(p, s)| p == project && !s.is_empty())
        else {
            return String::new();
        };
        let shown: Vec<&str> = subjects.iter().take(2).map(String::as_str).collect();
        let n = subjects.len();
        format!(
            ". {} ({n} commit{})",
            shown.join(", "),
            if n == 1 { "" } else { "s" }
        )
    }

    /// Gathers today's commits from the code folders and meetings from the
    /// calendar. Slow (runs git); call off the UI thread.
    pub fn gather(app: &AppHandle) -> Self {
        let (roots, meetings) = {
            let state = app.state::<AppState>();
            let roots = crate::repo_roots(&lock(&state.settings));
            let meetings = sidekick_sensors::calendar::on_day(
                &lock(&state.calendar).meetings,
                Local::now().date_naive(),
            )
            .into_iter()
            .map(|m| m.title)
            .collect();
            (roots, meetings)
        };
        let commits = roots
            .iter()
            .flat_map(|r| sidekick_sensors::repos::find_repos(r))
            .filter_map(|repo| {
                let subjects = sidekick_actions::gitflow::today_commits(&repo);
                let name = repo.file_name()?.to_string_lossy().into_owned();
                (!subjects.is_empty()).then_some((name, subjects))
            })
            .collect();
        Self { commits, meetings }
    }
}

/// The day's summary event: time per project (or app), with a
/// plain-text version ready to paste into a standup or timesheet.
pub fn day_summary(rows: &[sidekick_core::AppTime], done: &Done) -> Option<Event> {
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
        .map(|(n, s)| format!("- {n}: {}{}", human(*s), done.about(n)))
        .collect();
    let mut text = format!("Today ({} total)\n{}", human(total), lines.join("\n"));
    // Work in projects you did not have open long enough to make the list.
    for (project, _) in &done.commits {
        if !by_name.iter().any(|(n, s)| n == project && *s >= 5 * 60) {
            text.push_str(&format!(
                "\n- {project}: {}",
                done.about(project).trim_start_matches(". ")
            ));
        }
    }
    if !done.meetings.is_empty() {
        text.push_str(&format!("\nMeetings: {}", done.meetings.join(", ")));
    }
    Some(Event::new(
        DAY_SUMMARY,
        "time",
        serde_json::json!({ "total_human": human(total), "top": top.join(", "), "text": text }),
    ))
}

/// The hour you were last at the PC, per day ("2026-10-08" -> 19), kept
/// for two weeks to learn when your day really ends.
const LAST_HOURS: &str = "day-ends.json";
const DEFAULT_DAY_END: u32 = 18;

fn last_hours(app: &AppHandle) -> std::collections::BTreeMap<String, u32> {
    std::fs::read_to_string(app.state::<AppState>().data_dir.join(LAST_HOURS))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn note_active_hour(app: &AppHandle, day: &str, hour: u32) {
    let mut hours = last_hours(app);
    if hours.get(day) == Some(&hour) {
        return;
    }
    hours.insert(day.to_owned(), hour);
    while hours.len() > 14 {
        let first = hours.keys().next().cloned().unwrap_or_default();
        hours.remove(&first);
    }
    if let Ok(json) = serde_json::to_string(&hours) {
        let _ = std::fs::write(app.state::<AppState>().data_dir.join(LAST_HOURS), json);
    }
}

/// When your day usually ends: the middle of your last hours at the PC,
/// once there are five days to go on (today left out, it is not over).
pub fn learned_day_end(
    hours: &std::collections::BTreeMap<String, u32>,
    today: &str,
) -> Option<u32> {
    let mut past: Vec<u32> = hours
        .iter()
        .filter(|(d, _)| d.as_str() != today)
        .map(|(_, h)| *h)
        .collect();
    if past.len() < 5 {
        return None;
    }
    past.sort_unstable();
    Some(past[past.len() / 2])
}

/// The learned day end, when there is one (for Memory).
pub fn learned_end(app: &AppHandle) -> Option<u32> {
    learned_day_end(&last_hours(app), &today())
}

pub fn forget_day_ends(app: &AppHandle) {
    let _ = std::fs::remove_file(app.state::<AppState>().data_dir.join(LAST_HOURS));
}

/// The day-end hour: yours if set, else the one learned from your days.
pub fn day_end(app: &AppHandle) -> u32 {
    let set = lock(&app.state::<AppState>().settings).end_of_day_hour;
    if set != DEFAULT_DAY_END || !crate::learned::on(app) {
        return set;
    }
    learned_day_end(&last_hours(app), &today()).unwrap_or(set)
}

pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut summarized: Option<chrono::NaiveDate> = None;
        loop {
            tokio::time::sleep(FLUSH_EVERY).await;
            let now = Instant::now();
            let state = app.state::<AppState>();
            // Paused: the open span ends here, so paused time is not counted.
            if crate::state::is_paused(&app) {
                if let Some(mut span) = lock(&state.tracker.current).take() {
                    write(&app, &mut span, now);
                }
                continue;
            }
            let mut long_session = None;
            {
                let mut current = lock(&state.tracker.current);
                if let Some(span) = current.as_mut() {
                    write(&app, span, now);
                    note_active_hour(&app, &today(), chrono::Timelike::hour(&Local::now()));
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
            let eod = day_end(&app);
            if chrono::Timelike::hour(&local) >= eod && summarized != Some(local.date_naive()) {
                summarized = Some(local.date_naive());
                let rows = lock(&state.storage)
                    .time_for_day(&today())
                    .unwrap_or_default();
                let app2 = app.clone();
                let done = tokio::task::spawn_blocking(move || Done::gather(&app2))
                    .await
                    .unwrap_or_default();
                if let Some(e) = day_summary(&rows, &done) {
                    state.bus.publish(e);
                }
            }
            if let Some((name, project)) = long_session {
                let minutes = FOCUS_AFTER.as_secs() / 60;
                let dnd = tokio::task::spawn_blocking(sidekick_actions::pc::read_state)
                    .await
                    .map(|s| s.do_not_disturb)
                    .unwrap_or_default();
                state.bus.publish(Event::new(
                    LONG_SESSION,
                    "time",
                    serde_json::json!({
                        "app": name,
                        "project": project,
                        "minutes": minutes,
                        "dnd": dnd.as_str(),
                    }),
                ));
            }
        }
    });
}

#[cfg(test)]
mod tests {

    #[test]
    fn learns_when_the_day_ends() {
        let mut hours = std::collections::BTreeMap::new();
        for (d, h) in [("01", 17), ("02", 19), ("03", 19), ("04", 20)] {
            hours.insert(format!("2026-10-{d}"), h);
        }
        assert_eq!(
            learned_day_end(&hours, "2026-10-08"),
            None,
            "four days is too few"
        );
        hours.insert("2026-10-05".into(), 19);
        hours.insert("2026-10-08".into(), 9);
        assert_eq!(learned_day_end(&hours, "2026-10-08"), Some(19));
    }

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
        let done = Done {
            commits: vec![
                (
                    "sidekick".into(),
                    vec!["Fix tool calls".into(), "Add eval".into(), "Docs".into()],
                ),
                ("site".into(), vec!["New hero".into()]),
            ],
            meetings: vec!["Standup".into()],
        };
        let e = day_summary(&rows, &done).unwrap();
        assert_eq!(e.payload["total_human"], "3 h 12 min");
        assert_eq!(
            e.payload["top"],
            "sidekick 2 h 30 min, Chrome 40 min, Slack 2 min"
        );
        let text = e.payload["text"].as_str().unwrap();
        assert!(text.contains("- sidekick: 2 h 30 min. Fix tool calls, Add eval (3 commits)"));
        assert!(text.contains("- site: New hero (1 commit)"));
        assert!(text.contains("Meetings: Standup"));
        assert!(!text.contains("Slack"), "under five minutes is left out");
        assert!(day_summary(&[row("Code", "x", 600)], &Done::default()).is_none());
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
        assert_eq!(
            project_from_title("cursor.exe", "toasts.rs - sidekick [WSL: Ubuntu] - Cursor"),
            "sidekick"
        );
        assert_eq!(project_from_title("chrome.exe", "Docs - Google Chrome"), "");
        assert_eq!(project_from_title("code.exe", "Visual Studio Code"), "");
    }
}
