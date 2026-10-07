//! Instant results in Ask: installed apps and files whose name matches what
//! is typed, found without any model. The app list is read from the Start
//! menu when Ask opens and kept for a while; files get a short time budget.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::state::{AppState, lock};

/// How long the Start menu list is trusted before it is read again.
const APPS_FRESH: Duration = Duration::from_secs(10 * 60);
/// Files must not hold up typing.
const FILE_BUDGET: Duration = Duration::from_millis(150);
const MAX_EACH: usize = 3;

/// When the Start menu list was read, and the list: (name, app id).
type AppList = Option<(Instant, Vec<(String, String)>)>;
static APPS: Mutex<AppList> = Mutex::new(None);

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppHit {
    pub name: String,
    pub id: String,
    /// Minutes in this app over the last week, for ranking and the hint.
    pub minutes: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FileHit {
    pub name: String,
    pub path: String,
    pub folder: bool,
    /// The folder it is in, short: "Documents", "Downloads".
    pub place: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Results {
    pub apps: Vec<AppHit>,
    pub files: Vec<FileHit>,
}

/// Reads the Start menu list in the background when it is old or missing.
pub fn refresh() {
    let stale = lock(&APPS)
        .as_ref()
        .is_none_or(|(at, _)| at.elapsed() > APPS_FRESH);
    if stale {
        std::thread::spawn(|| {
            if let Ok(list) = sidekick_actions::pc::installed_apps() {
                *lock(&APPS) = Some((Instant::now(), list));
            }
        });
    }
}

/// Apps and files named like `query`, best first.
pub fn find(app: &AppHandle, query: &str) -> Results {
    let query = query.trim();
    if query.chars().count() < 2 {
        return Results::default();
    }
    refresh();
    let usage = week_usage(app);
    let apps = lock(&APPS)
        .as_ref()
        .map(|(_, list)| rank_apps(list, query, &usage))
        .unwrap_or_default();
    let files = find_files(app, query);
    Results { apps, files }
}

/// Matching apps, the ones used most this week first; among apps not used,
/// the closest name first.
pub fn rank_apps(list: &[(String, String)], query: &str, usage: &[(String, i64)]) -> Vec<AppHit> {
    let minutes = |name: &str| {
        let n = name.to_lowercase();
        let secs = |f: &dyn Fn(&str) -> bool| {
            usage
                .iter()
                .filter(|(a, _)| f(&a.to_lowercase()))
                .map(|(_, s)| *s)
                .max()
        };
        // The same name first; a usage entry inside a longer name only counts
        // when nothing has the exact name.
        secs(&|a| a == n)
            .or_else(|| {
                secs(&|a| {
                    a.len() >= 4
                        && n.starts_with(a)
                        && !list.iter().any(|(o, _)| o.to_lowercase() == a)
                })
            })
            .unwrap_or(0)
            / 60
    };
    let mut hits: Vec<AppHit> = sidekick_actions::pc::match_apps(list, query)
        .into_iter()
        .take(8)
        .map(|(name, id)| AppHit {
            name: name.clone(),
            id: id.clone(),
            minutes: minutes(name),
        })
        .collect();
    // Stable sort keeps the name order among equals.
    hits.sort_by_key(|h| std::cmp::Reverse(h.minutes));
    hits.truncate(MAX_EACH);
    hits
}

fn week_usage(app: &AppHandle) -> Vec<(String, i64)> {
    let since = (chrono::Local::now() - chrono::Duration::days(7))
        .format("%Y-%m-%d")
        .to_string();
    let state = app.state::<AppState>();
    let storage = lock(&state.storage);
    storage.time_by_app_since(&since).unwrap_or_default()
}

fn find_files(app: &AppHandle, query: &str) -> Vec<FileHit> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    let configured: Vec<PathBuf> = lock(&app.state::<AppState>().settings)
        .code_folders
        .iter()
        .map(PathBuf::from)
        .collect();
    let code = if configured.is_empty() {
        sidekick_sensors::repos::ReposSensor::default_roots()
    } else {
        configured
    };
    let roots = crate::find::roots(&home, &code);
    crate::find::find(&roots, query, None, FILE_BUDGET)
        .into_iter()
        .take(MAX_EACH)
        .map(|f| FileHit {
            name: f
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            place: f
                .path
                .parent()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            path: f.path.to_string_lossy().into_owned(),
            folder: f.folder,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list() -> Vec<(String, String)> {
        [
            ("Cursor", "cursor.id"),
            ("Cursor Agent", "cursor.agent"),
            ("Slack", "slack.id"),
            ("Microsoft Teams", "teams.id"),
        ]
        .iter()
        .map(|(n, i)| ((*n).into(), (*i).into()))
        .collect()
    }

    #[test]
    fn matches_by_name_and_ranks_by_use() {
        let usage = vec![
            ("Cursor Agent".to_owned(), 3600_i64),
            ("Cursor".to_owned(), 60),
        ];
        let hits = rank_apps(&list(), "curs", &usage);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].name, "Cursor Agent");
        assert!(rank_apps(&list(), "zzz", &usage).is_empty());
    }
}
