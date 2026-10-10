//! Projects: when a repo comes to the front in an
//! editor, report its branch and how far behind it is, a missing `.env`,
//! and whether Docker is needed. Also lists repos for the launcher.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use chrono::{Local, Utc};
use serde::Serialize;
use serde_json::Value;
use sidekick_core::Event;
use sidekick_sensors::ReposSensor;
use sidekick_sensors::repos::{find_repos, status};
use tauri::{AppHandle, Manager};

use crate::state::{AppState, lock};

/// How far back time-tracking ranks the project picker.
const RANK_DAYS: i64 = 30;

pub const REPO_OPENED: &str = "dev.repo_opened";
/// The same project is checked again after this long.
const RECHECK: Duration = Duration::from_secs(3 * 60 * 60);
const LIST_TTL: Duration = Duration::from_secs(10 * 60);

static CHECKED: Mutex<Option<HashMap<String, Instant>>> = Mutex::new(None);
static LIST: Mutex<Option<(Instant, Vec<PathBuf>)>> = Mutex::new(None);

pub fn roots(app: &AppHandle) -> Vec<PathBuf> {
    let folders = lock(&app.state::<AppState>().settings).code_folders.clone();
    if folders.is_empty() {
        ReposSensor::default_roots()
    } else {
        folders.iter().map(Into::into).collect()
    }
}

/// Repos under the code folders, cached for a few minutes.
pub fn list(app: &AppHandle) -> Vec<PathBuf> {
    if let Ok(cache) = LIST.lock()
        && let Some((at, repos)) = cache.as_ref()
        && at.elapsed() < LIST_TTL
    {
        return repos.clone();
    }
    let mut repos: Vec<PathBuf> = roots(app).iter().flat_map(|r| find_repos(r)).collect();
    repos.sort_by_key(|p| p.file_name().map(|n| n.to_ascii_lowercase()));
    repos.dedup();
    if let Ok(mut cache) = LIST.lock() {
        *cache = Some((Instant::now(), repos.clone()));
    }
    repos
}

fn due(project: &str) -> bool {
    let Ok(mut checked) = CHECKED.lock() else {
        return false;
    };
    let map = checked.get_or_insert_with(HashMap::new);
    match map.get(project) {
        Some(at) if at.elapsed() < RECHECK => false,
        _ => {
            map.insert(project.to_owned(), Instant::now());
            true
        }
    }
}

/// A window came to the front.
pub fn on_window(app: &AppHandle, payload: &Value) {
    let exe = payload["exe"].as_str().unwrap_or_default();
    let title = payload["title"].as_str().unwrap_or_default();
    let project = crate::timetrack::project_from_title(exe, title);
    if project.is_empty() {
        return;
    }
    {
        let state = app.state::<AppState>();
        let s = lock(&state.settings);
        if s.pause.is_active(Utc::now()) || !s.sensor_enabled(ReposSensor::ID) {
            return;
        }
    }
    if !due(&project.to_lowercase()) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(path) = list(&app).into_iter().find(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(&project))
        }) else {
            return;
        };
        let mut payload = status(&path);
        payload["docker_running"] = Value::Bool(docker_running());
        if worth_saying(&payload) {
            app.state::<AppState>()
                .bus
                .publish(Event::new(REPO_OPENED, ReposSensor::ID, payload));
        }
    });
}

/// Only speak up when there is something to do.
fn worth_saying(p: &Value) -> bool {
    p["behind"].as_u64().unwrap_or(0) > 0
        || p["env_missing"].as_bool().unwrap_or(false)
        || (p["docker_needed"].as_bool().unwrap_or(false)
            && !p["docker_running"].as_bool().unwrap_or(true))
}

fn docker_running() -> bool {
    let mut sys = sysinfo::System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    sys.processes().values().any(|p| {
        let name = p.name().to_string_lossy().to_ascii_lowercase();
        name.starts_with("docker desktop")
            || name.starts_with("com.docker.backend")
            || name == "dockerd"
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectInfo {
    pub name: String,
    pub path: String,
}

pub fn infos(app: &AppHandle) -> Vec<ProjectInfo> {
    let since = (Local::now() - chrono::Duration::days(RANK_DAYS))
        .format("%Y-%m-%d")
        .to_string();
    // Lowercase name → (last day worked, total secs). Recent first, then most time.
    let rank: HashMap<String, (String, i64)> = lock(&app.state::<AppState>().storage)
        .time_by_project_since(&since)
        .unwrap_or_default()
        .into_iter()
        .map(|(name, secs, last)| (name.to_ascii_lowercase(), (last, secs)))
        .collect();
    let mut out: Vec<ProjectInfo> = list(app)
        .into_iter()
        .map(|p| ProjectInfo {
            name: p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            path: p.to_string_lossy().into_owned(),
        })
        .collect();
    out.sort_by(|a, b| {
        let ra = rank.get(&a.name.to_ascii_lowercase());
        let rb = rank.get(&b.name.to_ascii_lowercase());
        match (ra, rb) {
            (Some((da, sa)), Some((db, sb))) => db
                .cmp(da)
                .then(sb.cmp(sa))
                .then_with(|| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase())),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()),
        }
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speaks_only_when_there_is_something_to_do() {
        let base = serde_json::json!({ "behind": 0, "env_missing": false, "docker_needed": false, "docker_running": false });
        assert!(!worth_saying(&base));
        let mut behind = base.clone();
        behind["behind"] = 2.into();
        assert!(worth_saying(&behind));
        let mut docker = base.clone();
        docker["docker_needed"] = true.into();
        assert!(worth_saying(&docker));
        docker["docker_running"] = true.into();
        assert!(!worth_saying(&docker));
    }
}
