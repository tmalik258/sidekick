//! Instant results in Ask: installed apps and files whose name matches what
//! is typed, found without any model. Both lists are built in the background
//! (at start and when Ask opens) and matched in memory, so results show on
//! the first keystroke instead of after a disk walk or a PowerShell call.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::state::{AppState, lock};

/// How long the Start menu list is trusted before it is read again.
const APPS_FRESH: Duration = Duration::from_secs(10 * 60);
/// How long the file list is trusted before it is walked again.
const FILES_FRESH: Duration = Duration::from_secs(5 * 60);
/// Before the file list exists, a live walk gets this long.
const FILE_BUDGET: Duration = Duration::from_millis(80);
/// The background walk stops after this many names or this long.
const INDEX_CAP: usize = 200_000;
const INDEX_BUDGET: Duration = Duration::from_secs(20);
const MAX_EACH: usize = 3;

/// When the Start menu list was read, and the list: (name, app id).
type AppList = Option<(Instant, Vec<(String, String)>)>;
static APPS: Mutex<AppList> = Mutex::new(None);

/// One file or folder in the background list, its name ready to match.
struct Entry {
    key: String,
    path: PathBuf,
    folder: bool,
}
/// When the file list was built, and the list (nearest folders first).
static FILES: Mutex<Option<(Instant, Vec<Entry>)>> = Mutex::new(None);
/// A refresh is running; another one is not started.
static APPS_BUSY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static FILES_BUSY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

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
    /// "18% of 2450" worked out: "441".
    pub calc: Option<String>,
}

/// Rebuilds the app and file lists in the background when old or missing.
/// The old lists keep answering meanwhile.
pub fn refresh(app: &AppHandle) {
    use std::sync::atomic::Ordering;
    let stale = lock(&APPS)
        .as_ref()
        .is_none_or(|(at, _)| at.elapsed() > APPS_FRESH);
    if stale && !APPS_BUSY.swap(true, Ordering::SeqCst) {
        std::thread::spawn(|| {
            if let Ok(list) = sidekick_actions::pc::installed_apps() {
                *lock(&APPS) = Some((Instant::now(), list));
            }
            APPS_BUSY.store(false, Ordering::SeqCst);
        });
    }
    let stale = lock(&FILES)
        .as_ref()
        .is_none_or(|(at, _)| at.elapsed() > FILES_FRESH);
    if stale && !FILES_BUSY.swap(true, Ordering::SeqCst) {
        let roots = file_roots(app);
        std::thread::spawn(move || {
            let list = index(&roots);
            *lock(&FILES) = Some((Instant::now(), list));
            FILES_BUSY.store(false, Ordering::SeqCst);
        });
    }
}

/// Every name under `roots`, nearest first, skipping what `find` skips.
fn index(roots: &[PathBuf]) -> Vec<Entry> {
    let started = Instant::now();
    let mut out = Vec::new();
    crate::find::walk(roots, |path, name, folder| {
        out.push(Entry {
            key: crate::find::key(name),
            path: path.to_path_buf(),
            folder,
        });
        out.len() < INDEX_CAP && started.elapsed() < INDEX_BUDGET
    });
    out
}

/// Apps and files named like `query`, best first.
pub fn find(app: &AppHandle, query: &str) -> Results {
    let query = query.trim();
    if query.chars().count() < 2 {
        return Results::default();
    }
    if let Some(answer) = sidekick_calc::calculate(query) {
        return Results {
            calc: Some(sidekick_calc::format(answer)),
            ..Results::default()
        };
    }
    refresh(app);
    let usage = week_usage(app);
    let apps = lock(&APPS)
        .as_ref()
        .map(|(_, list)| rank_apps(list, query, &usage))
        .unwrap_or_default();
    let files = find_files(app, query);
    Results {
        apps,
        files,
        calc: None,
    }
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

/// The user's folders and code folders. App data is left out: it is huge and
/// rarely what someone types a name for.
fn file_roots(app: &AppHandle) -> Vec<PathBuf> {
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
    crate::find::roots(&home, &code)
        .into_iter()
        .filter(|r| !r.ends_with("Roaming") && !r.ends_with("Local"))
        .collect()
}

fn find_files(app: &AppHandle, query: &str) -> Vec<FileHit> {
    let words = crate::find::words(query);
    if words.is_empty() {
        return Vec::new();
    }
    let mut hits: Vec<(PathBuf, bool)> = match lock(&FILES).as_ref() {
        Some((_, list)) => list
            .iter()
            .filter(|e| words.iter().all(|w| e.key.contains(w.as_str())))
            .take(MAX_EACH)
            .map(|e| (e.path.clone(), e.folder))
            .collect(),
        // Not built yet: a short live walk.
        None => crate::find::find(&file_roots(app), query, None, FILE_BUDGET)
            .into_iter()
            .map(|f| (f.path, f.folder))
            .collect(),
    };
    // Room left: files anywhere on the PC, from the name index.
    if hits.len() < MAX_EACH {
        for h in crate::names::search(query, MAX_EACH * 2).unwrap_or_default() {
            if hits.len() >= MAX_EACH {
                break;
            }
            if !hits.iter().any(|(p, _)| *p == h.path) {
                hits.push((h.path, h.folder));
            }
        }
    }
    hits.into_iter()
        .take(MAX_EACH)
        .map(|(path, folder)| FileHit {
            name: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            place: path
                .parent()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            path: path.to_string_lossy().into_owned(),
            folder,
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

    /// Matching the background list must stay instant even for a big home
    /// folder: 20,000 names in well under a frame.
    #[test]
    fn matching_the_file_list_is_instant() {
        let dir = std::env::temp_dir().join(format!("sk-instant-{}", std::process::id()));
        for i in 0..200 {
            let sub = dir.join(format!("project-{i}"));
            std::fs::create_dir_all(&sub).unwrap();
            for j in 0..100 {
                std::fs::write(sub.join(format!("note-{j}.txt")), "").unwrap();
            }
        }
        std::fs::write(dir.join("project-7").join("Cursor settings.json"), "").unwrap();
        let list = index(std::slice::from_ref(&dir));
        assert!(list.len() > 20_000);
        let started = Instant::now();
        let words = crate::find::words("cursor sett");
        let hit: Vec<_> = list
            .iter()
            .filter(|e| words.iter().all(|w| e.key.contains(w.as_str())))
            .collect();
        let took = started.elapsed();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(hit.len(), 1);
        assert!(took < Duration::from_millis(16), "matching took {took:?}");
    }
}
