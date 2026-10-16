//! Live operations only; never persisted across process restarts.
use crate::state::lock;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Running,
    Downloading,
    Verifying,
    Installing,
    AwaitingUser,
    Completed,
    Failed,
    Unconfirmed,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Activity {
    pub id: String,
    pub source: String,
    pub parent_id: Option<String>,
    pub label: String,
    pub phase: Phase,
    pub started_at: i64,
    pub last_progress_at: i64,
    pub transferred: Option<u64>,
    pub total: Option<u64>,
    pub outcome: Option<String>,
    pub dismissed: bool,
    pub revision: u64,
    pub destination: Option<String>,
    #[serde(skip)]
    progress_key: Option<String>,
}
#[derive(Clone, Default, Serialize)]
pub struct Snapshot {
    pub revision: u64,
    pub activities: Vec<Activity>,
}
#[derive(Default)]
struct Registry {
    revision: u64,
    entries: BTreeMap<String, Activity>,
}
static REGISTRY: Mutex<Registry> = Mutex::new(Registry {
    revision: 0,
    entries: BTreeMap::new(),
});
static DOWNLOAD_REVISIONS: Mutex<BTreeMap<String, u64>> = Mutex::new(BTreeMap::new());
static EXCLUDED_DOWNLOADS: Mutex<BTreeMap<String, i64>> = Mutex::new(BTreeMap::new());

fn normalize(path: &str) -> String {
    path.replace('\\', "/").to_lowercase()
}

pub fn exclude_download(app: &AppHandle, event: &duskie_core::Event) {
    if event.kind != "browser.download" {
        return;
    }
    let Some(path) = event.payload["filename"].as_str() else {
        return;
    };
    let path = normalize(path);
    let now = chrono::Utc::now().timestamp_millis();
    {
        let mut excluded = lock(&EXCLUDED_DOWNLOADS);
        excluded.retain(|_, until| *until > now);
        excluded.insert(path.clone(), now + 120_000);
    }
    let mut registry = lock(&REGISTRY);
    let before = registry.entries.len();
    registry.entries.retain(|_, a| {
        !a.destination
            .as_deref()
            .is_some_and(|p| normalize(p) == path)
    });
    if registry.entries.len() == before {
        return;
    }
    registry.revision += 1;
    let value = snapshot(&registry);
    drop(registry);
    let _ = app.emit("activity://changed", value);
}

pub fn download_excluded(event: &duskie_core::Event) -> bool {
    if !matches!(
        event.kind.as_str(),
        "file.download_progress" | "file.download_completed"
    ) {
        return false;
    }
    let path = event.payload["destination"]
        .as_str()
        .or_else(|| event.payload["path"].as_str())
        .unwrap_or_default();
    let mut excluded = lock(&EXCLUDED_DOWNLOADS);
    let now = chrono::Utc::now().timestamp_millis();
    excluded.retain(|_, until| *until > now);
    excluded.contains_key(&normalize(path))
}
fn snapshot(r: &Registry) -> Snapshot {
    Snapshot {
        revision: r.revision,
        activities: r.entries.values().cloned().collect(),
    }
}
#[tauri::command]
pub fn activity_snapshot() -> Snapshot {
    snapshot(&lock(&REGISTRY))
}
#[tauri::command]
pub fn activity_reconcile(app: AppHandle) -> bool {
    use tauri::Manager;
    let registry = lock(&REGISTRY);
    if registry.entries.values().any(|a| {
        matches!(a.source.as_str(), "action" | "open" | "chat" | "agent")
            && !matches!(a.phase, Phase::Completed | Phase::Failed)
    }) || !lock(&app.state::<crate::state::AppState>().chats).is_empty()
        || crate::mascot::current(&app) != duskie_core::MascotState::Working
    {
        return false;
    }
    crate::mascot::force(&app, duskie_core::MascotState::Idle);
    drop(registry);
    crate::suggestions::resume(&app);
    true
}
pub fn restart(id: &str) {
    lock(&REGISTRY).entries.remove(id);
}

/// Drops an activity without a Completed flash (cancelled / never shown).
pub fn forget(app: &AppHandle, id: &str) {
    let mut r = lock(&REGISTRY);
    if r.entries.remove(id).is_none() {
        return;
    }
    r.revision += 1;
    let value = snapshot(&r);
    drop(r);
    let _ = app.emit("activity://changed", value);
}
#[tauri::command]
pub fn activity_open_start(app: AppHandle, id: String, label: String) -> Result<(), String> {
    if id.len() > 128 || label.len() > 1024 {
        return Err("Invalid activity".into());
    }
    report(&app, &id, "open", &label, Phase::Running, None, None, None);
    Ok(())
}
#[tauri::command]
pub fn activity_open_finish(app: AppHandle, id: String, ok: bool, outcome: String) {
    let old = lock(&REGISTRY)
        .entries
        .get(&id)
        .filter(|a| a.source == "open")
        .cloned();
    if let Some(a) = old {
        report(
            &app,
            &id,
            "open",
            &a.label,
            if ok { Phase::Completed } else { Phase::Failed },
            None,
            None,
            Some(outcome.chars().take(4096).collect()),
        );
    }
}

#[allow(clippy::too_many_arguments)] // matches the activity event payload fields
pub fn report(
    app: &AppHandle,
    id: &str,
    source: &str,
    label: &str,
    phase: Phase,
    bytes: Option<u64>,
    total: Option<u64>,
    outcome: Option<String>,
) {
    let mut r = lock(&REGISTRY);
    let now = chrono::Utc::now().timestamp_millis();
    let old = r.entries.get(id);
    if old.is_some_and(|a| matches!(a.phase, Phase::Completed | Phase::Failed)) {
        return;
    }
    let meaningful = meaningful_progress(old, &phase, label, bytes, &outcome);
    if old.is_some_and(|a| !meaningful && a.total == total && a.transferred == bytes) {
        return;
    }
    let started_at = old.map_or(now, |a| a.started_at);
    let last_progress_at = if meaningful {
        now
    } else {
        old.unwrap().last_progress_at
    };
    let dismissed = !meaningful && old.is_some_and(|a| a.dismissed);
    let destination = old.and_then(|a| a.destination.clone());
    let parent_id = old.and_then(|a| a.parent_id.clone());
    let progress_key = outcome.clone();
    let outcome = if matches!(phase, Phase::Completed | Phase::Failed) {
        outcome
    } else {
        None
    };
    r.revision += 1;
    let revision = r.revision;
    r.entries.insert(
        id.into(),
        Activity {
            id: id.into(),
            source: source.into(),
            parent_id,
            label: label.into(),
            phase,
            started_at,
            last_progress_at,
            transferred: bytes,
            total,
            outcome,
            dismissed,
            revision,
            destination,
            progress_key,
        },
    );
    // Keep completed history bounded, without evicting running operations.
    if r.entries.len() > 256 {
        let remove = r
            .entries
            .values()
            .filter(|a| matches!(a.phase, Phase::Completed | Phase::Failed))
            .min_by_key(|a| a.revision)
            .map(|a| a.id.clone());
        if let Some(id) = remove {
            r.entries.remove(&id);
        }
    }
    let value = snapshot(&r);
    drop(r);
    let _ = app.emit("activity://changed", value);
}

fn meaningful_progress(
    old: Option<&Activity>,
    phase: &Phase,
    label: &str,
    bytes: Option<u64>,
    outcome: &Option<String>,
) -> bool {
    old.is_none_or(|a| {
        &a.phase != phase
            || bytes.unwrap_or(0) > a.transferred.unwrap_or(0)
            || a.label != label
            || &a.progress_key != outcome
    })
}
fn metadata(
    app: &AppHandle,
    id: &str,
    parent: Option<String>,
    destination: Option<String>,
    started_at: Option<i64>,
) {
    let mut r = lock(&REGISTRY);
    let Some(old) = r.entries.get(id) else {
        return;
    };
    if old.parent_id == parent
        && old.destination == destination
        && started_at.is_none_or(|at| old.started_at == at)
    {
        return;
    }
    r.revision += 1;
    let revision = r.revision;
    if let Some(a) = r.entries.get_mut(id) {
        a.parent_id = parent;
        a.destination = destination;
        a.revision = revision;
        if let Some(at) = started_at {
            a.started_at = at;
        }
    }
    let value = snapshot(&r);
    drop(r);
    let _ = app.emit("activity://changed", value);
}

#[tauri::command]
pub fn activity_dismiss(app: AppHandle, id: String) {
    let mut r = lock(&REGISTRY);
    r.revision += 1;
    let revision = r.revision;
    if let Some(a) = r.entries.get_mut(&id) {
        a.dismissed = true;
        a.revision = revision;
    }
    let value = snapshot(&r);
    drop(r);
    let _ = app.emit("activity://changed", value);
}
#[tauri::command]
pub fn activity_check(app: AppHandle, id: String) -> Snapshot {
    crate::setup_progress::check(&id);
    let source = lock(&REGISTRY).entries.get(&id).map(|a| a.source.clone());
    if source.as_deref() == Some("browser") {
        use tauri::Manager;
        app.state::<crate::state::AppState>()
            .browser
            .send(serde_json::json!({"type":"check_downloads"}));
    }
    // Owned sources already retain their current operation/progress. Reading
    // them never invokes install, download, or execution again.
    if source.as_deref() == Some("voice") {
        let _ = crate::voice::status(&app);
    }
    if source.as_deref() == Some("update") {
        let _ = crate::updates::progress();
    }
    activity_snapshot()
}

pub struct Guard {
    app: AppHandle,
    id: String,
    label: String,
    finished: bool,
}
impl Guard {
    pub fn start(app: &AppHandle, label: &str) -> Self {
        let id = ulid::Ulid::new().to_string();
        report(app, &id, "action", label, Phase::Running, None, None, None);
        Self {
            app: app.clone(),
            id,
            label: label.into(),
            finished: false,
        }
    }
    pub fn finish(&mut self, ok: bool, message: String) {
        report(
            &self.app,
            &self.id,
            "action",
            &self.label,
            if ok { Phase::Completed } else { Phase::Failed },
            None,
            None,
            Some(message),
        );
        self.finished = true;
    }
}
impl Drop for Guard {
    fn drop(&mut self) {
        if !self.finished {
            self.finish(
                false,
                "Operation exited before confirming its outcome".into(),
            );
        }
    }
}

pub fn observe(app: &AppHandle, event: &duskie_core::Event) {
    let p = &event.payload;
    if event.kind == "browser.download" {
        if let Some(path) = p["filename"].as_str() {
            lock(&EXCLUDED_DOWNLOADS).remove(&normalize(path));
        }
        let id = format!(
            "browser:{}:{}",
            p["instance"].as_str().unwrap_or_default(),
            p["id"]
        );
        let sequence = p["sequence"].as_u64().unwrap_or(0);
        {
            let mut revisions = lock(&DOWNLOAD_REVISIONS);
            if revisions
                .get(&id)
                .is_some_and(|previous| sequence <= *previous)
            {
                return;
            }
            revisions.insert(id.clone(), sequence);
            if revisions.len() > 4096 {
                let oldest = revisions
                    .iter()
                    .min_by_key(|(_, revision)| *revision)
                    .map(|(key, _)| key.clone());
                if let Some(key) = oldest {
                    revisions.remove(&key);
                }
            }
        }
        if p["state"] == "in_progress"
            && lock(&REGISTRY)
                .entries
                .get(&id)
                .is_some_and(|a| a.phase == Phase::Failed)
        {
            restart(&id);
        }
        let phase = match p["state"].as_str() {
            Some("complete") => Phase::Completed,
            Some("interrupted") => Phase::Failed,
            _ if p["paused"] == true => Phase::AwaitingUser,
            _ => Phase::Downloading,
        };
        let name = std::path::Path::new(p["filename"].as_str().unwrap_or("download"))
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        report(
            app,
            &id,
            "browser",
            &format!(
                "Downloading {name} in {}",
                p["browser"].as_str().unwrap_or("browser")
            ),
            phase,
            p["bytes"].as_u64(),
            p["total"].as_u64().filter(|n| *n > 0),
            p["error"].as_str().map(str::to_owned),
        );
        let path = p["filename"].as_str().unwrap_or_default().to_owned();
        metadata(app, &id, None, Some(path.clone()), p["startedAt"].as_i64());
        // Browser totals and state supersede the partial-file fallback.
        let fallback = format!("file:{}", normalize(&path));
        if lock(&REGISTRY).entries.contains_key(&fallback) {
            activity_dismiss(app.clone(), fallback);
        }
    } else if event.kind == "file.download_progress" || event.kind == "file.download_completed" {
        let path = p["path"].as_str().unwrap_or_default();
        let destination = p["destination"].as_str().unwrap_or(path);
        if lock(&REGISTRY).entries.values().any(|a| {
            a.source == "browser"
                && (!matches!(a.phase, Phase::Completed | Phase::Failed)
                    || chrono::Utc::now().timestamp_millis() - a.last_progress_at < 5000)
                && a.destination
                    .as_deref()
                    .is_some_and(|d| normalize(d) == normalize(destination))
        }) {
            return;
        }
        let id = format!("file:{}", normalize(destination));
        let finished = lock(&REGISTRY)
            .entries
            .get(&id)
            .is_some_and(|a| a.phase == Phase::Completed);
        if event.kind == "file.download_progress" && p["missing"] != true && finished {
            restart(&id);
        }
        if event.kind == "file.download_completed" && !lock(&REGISTRY).entries.contains_key(&id) {
            return;
        }
        report(
            app,
            &id,
            "file",
            &format!("Downloading {}", p["name"].as_str().unwrap_or("file")),
            if event.kind == "file.download_completed" {
                Phase::Completed
            } else if p["missing"] == true {
                Phase::Unconfirmed
            } else {
                Phase::Downloading
            },
            p["size"].as_u64(),
            None,
            None,
        );
        metadata(app, &id, None, Some(destination.into()), None);
    }
}

pub fn agent_event(app: &AppHandle, session: &str, data: &serde_json::Value) {
    let id = format!("agent:{session}");
    let kind = data["kind"].as_str().unwrap_or_default();
    if matches!(kind, "usage" | "session" | "limit") {
        return;
    }
    if kind == "text" && data["text"].as_str().is_none_or(str::is_empty) {
        return;
    }
    let previous = lock(&REGISTRY).entries.get(&id).cloned();
    if kind == "working"
        && previous
            .as_ref()
            .is_some_and(|a| matches!(a.phase, Phase::Completed | Phase::Failed))
    {
        restart(&id);
    }
    let phase = match kind {
        "turn" | "ended" => {
            if data["error"].is_string() {
                Phase::Failed
            } else {
                Phase::Completed
            }
        }
        "ask" => Phase::AwaitingUser,
        _ => Phase::Running,
    };
    let agent = previous
        .as_ref()
        .and_then(|a| a.label.split(" · ").next())
        .unwrap_or("Agent");
    let label = if kind == "text" {
        format!("{agent} · Responding")
    } else if kind == "step"
        && data["state"]
            .as_str()
            .is_some_and(|state| state != "running")
    {
        format!(
            "{agent} · Step {}",
            data["state"].as_str().unwrap_or("finished")
        )
    } else {
        data["label"]
            .as_str()
            .map(|l| format!("{agent} · {l}"))
            .unwrap_or_else(|| {
                previous
                    .as_ref()
                    .map_or("Agent is working".into(), |a| a.label.clone())
            })
    };
    // Text, output, tool IDs, and state transitions are substantive. Usage
    // and repeated status reports are deliberately excluded.
    let signature = if matches!(kind, "text" | "step" | "plan") {
        Some(data.to_string())
    } else {
        data["error"].as_str().map(str::to_owned)
    };
    report(app, &id, "agent", &label, phase, None, None, signature);
    metadata(app, &id, Some(session.into()), None, None);
}

pub fn start(app: &AppHandle) {
    use tauri::Listener;
    for event_name in ["ai://delta", "ai://tool", "ai://done"] {
        let app = app.clone();
        let listener = app.clone();
        listener.listen(event_name, move |event| {
            let Ok(p) = serde_json::from_str::<serde_json::Value>(event.payload()) else {
                return;
            };
            let Some(chat) = p["id"].as_str() else {
                return;
            };
            if event_name == "ai://delta" && p["text"].as_str().is_none_or(str::is_empty) {
                return;
            }
            let id = format!("chat:{chat}");
            let old = lock(&REGISTRY).entries.get(&id).cloned();
            let phase = if event_name == "ai://done" {
                if p["error"].is_string() {
                    Phase::Failed
                } else {
                    Phase::Completed
                }
            } else {
                Phase::Running
            };
            let label = if event_name == "ai://delta" {
                "Answering your question".into()
            } else if event_name == "ai://tool" {
                p["label"].as_str().unwrap_or("Using a tool").to_owned()
            } else {
                old.as_ref()
                    .map_or("Answering your question".into(), |a| a.label.clone())
            };
            let signature = if event_name == "ai://delta"
                && p["text"].as_str().is_some_and(|s| !s.is_empty())
            {
                Some(format!(
                    "{}:{}",
                    old.as_ref().map_or(0, |a| a.revision),
                    p["text"]
                ))
            } else {
                p["error"].as_str().map(str::to_owned)
            };
            report(&app, &id, "chat", &label, phase, None, None, signature);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn downloading() -> Activity {
        Activity {
            id: "test".into(),
            source: "browser".into(),
            parent_id: None,
            label: "Downloading report.zip".into(),
            phase: Phase::Downloading,
            started_at: 0,
            last_progress_at: 0,
            transferred: Some(10),
            total: None,
            outcome: None,
            dismissed: false,
            revision: 1,
            destination: None,
            progress_key: None,
        }
    }
    #[test]
    fn heartbeats_and_total_discovery_do_not_count_as_progress() {
        let old = downloading();
        assert!(!meaningful_progress(
            Some(&old),
            &old.phase,
            &old.label,
            Some(10),
            &None
        ));
        assert!(!meaningful_progress(
            Some(&old),
            &old.phase,
            &old.label,
            Some(9),
            &None
        ));
        assert!(meaningful_progress(
            Some(&old),
            &old.phase,
            &old.label,
            Some(11),
            &None
        ));
        assert!(meaningful_progress(
            Some(&old),
            &Phase::AwaitingUser,
            &old.label,
            Some(10),
            &None
        ));
        assert!(meaningful_progress(
            Some(&old),
            &old.phase,
            &old.label,
            Some(10),
            &Some("New substantive output".into())
        ));
    }
    #[test]
    fn repeated_output_does_not_reset_inactivity() {
        let mut old = downloading();
        old.progress_key = Some("Still downloading".into());
        assert!(!meaningful_progress(
            Some(&old),
            &old.phase,
            &old.label,
            Some(10),
            &old.progress_key
        ));
    }
    #[test]
    fn excluded_browser_destinations_also_hide_matching_fallback_events() {
        let path = "C:/Downloads/private-activity-test.zip";
        let key = normalize(path);
        lock(&EXCLUDED_DOWNLOADS)
            .insert(key.clone(), chrono::Utc::now().timestamp_millis() + 120_000);
        let event = duskie_core::Event::new(
            "file.download_progress",
            "downloads",
            serde_json::json!({"destination": "c:\\downloads\\private-activity-test.zip"}),
        );
        assert!(download_excluded(&event));
        let other = duskie_core::Event::new(
            "file.download_progress",
            "downloads",
            serde_json::json!({"destination": "C:/Downloads/unrelated.zip"}),
        );
        assert!(!download_excluded(&other));
        lock(&EXCLUDED_DOWNLOADS).insert(key.clone(), 0);
        assert!(!download_excluded(&event));
        lock(&EXCLUDED_DOWNLOADS).remove(&key);
    }
}
