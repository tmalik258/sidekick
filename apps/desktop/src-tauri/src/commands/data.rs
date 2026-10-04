//! Search, clipboard, calendar, projects, routines, history and backups.

use super::*;

#[tauri::command]
pub fn events_recent(
    state: State<'_, AppState>,
    limit: Option<u32>,
) -> CmdResult<Vec<StoredEvent>> {
    lock(&state.storage)
        .recent_events(limit.unwrap_or(50).min(500))
        .map_err(|e| e.to_string())
}

/// Today's learned routine, for Settings.
#[tauri::command]
pub fn routines_today(app: AppHandle) -> Vec<crate::routines::Item> {
    crate::routines::today(&app)
}

/// Forgets every learned routine.
#[tauri::command]
pub fn routines_forget(app: AppHandle) -> CmdResult<usize> {
    crate::routines::forget(&app)
}

#[tauri::command]
pub fn actions_recent(
    state: State<'_, AppState>,
    limit: Option<u32>,
) -> CmdResult<Vec<ActionRecord>> {
    lock(&state.storage)
        .recent_actions(limit.unwrap_or(30).min(200))
        .map_err(|e| e.to_string())
}

/// Today's time per app and project, largest first.
#[tauri::command]
pub fn time_today(state: State<'_, AppState>) -> CmdResult<Vec<sidekick_core::AppTime>> {
    let day = chrono::Local::now().format("%Y-%m-%d").to_string();
    lock(&state.storage)
        .time_for_day(&day)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn search(app: AppHandle, query: String) -> Vec<sidekick_core::SearchHit> {
    crate::search::hybrid(&app, &query, &[], 30).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchStatus {
    items: u64,
    /// Items with an embedding (semantic search).
    embedded: u64,
    embed_error: Option<String>,
}

#[tauri::command]
pub fn search_status(app: AppHandle) -> CmdResult<SearchStatus> {
    let items = lock(&app.state::<AppState>().storage)
        .search_count()
        .map_err(|e| e.to_string())?;
    Ok(SearchStatus {
        items,
        embedded: crate::search::embedded_count(&app),
        embed_error: crate::search::embed_error(),
    })
}

#[tauri::command]
pub fn search_reindex(app: AppHandle) {
    crate::search::reindex_folders(&app);
}

/// Opens a search result: files are shown in Explorer, web pages open in the
/// default browser; nothing else is opened.
#[tauri::command]
pub async fn open_reference(app: AppHandle, source: String, reference: String) -> CmdResult<()> {
    let exec = executor(&app.state::<AppState>());
    let (action, args) = match source.as_str() {
        "file" | "download" | "screenshot" => {
            ("reveal_path", serde_json::json!({ "path": reference }))
        }
        "page" => ("open_url", serde_json::json!({ "url": reference })),
        _ => return Ok(()),
    };
    exec.run(action, &args)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Hands the reminder lead time to the calendar sensor.
pub fn sync_calendar(calendar: &sidekick_sensors::Calendar, settings: &Settings) {
    let mut c = calendar
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    c.remind_minutes = i64::from(settings.calendar.remind_minutes);
}

/// Today's meetings for Settings > Today.
#[tauri::command]
pub fn calendar_today(app: AppHandle) -> serde_json::Value {
    let state = app.state::<AppState>();
    let c = state
        .calendar
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let today = chrono::Local::now().date_naive();
    let meetings: Vec<serde_json::Value> = sidekick_sensors::calendar::on_day(&c.meetings, today)
        .iter()
        .map(|m| {
            serde_json::json!({
                "title": m.title,
                "start": m.start.with_timezone(&chrono::Local).format("%H:%M").to_string(),
                "end": m.end.with_timezone(&chrono::Local).format("%H:%M").to_string(),
                "joinUrl": m.join_url,
            })
        })
        .collect();
    serde_json::json!({ "meetings": meetings, "error": c.error, "sources": c.sources })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipItem {
    text: String,
    ts: String,
}

/// Recent clipboard text, newest first. Secrets are never in it.
#[tauri::command]
pub fn clipboard_history(app: AppHandle, limit: Option<u32>) -> Vec<ClipItem> {
    lock(&app.state::<AppState>().storage)
        .recent_items("clipboard", limit.unwrap_or(60).min(500))
        .unwrap_or_default()
        .into_iter()
        .map(|(_, _, text, ts)| ClipItem { text, ts })
        .collect()
}

/// Puts a history item back on the clipboard.
#[tauri::command]
pub async fn clipboard_copy(app: AppHandle, text: String) -> CmdResult<()> {
    executor(&app.state::<AppState>())
        .run("copy_text", &serde_json::json!({ "text": text }))
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Repos in the code folders, for the project launcher.
#[tauri::command]
pub async fn projects_list(app: AppHandle) -> Vec<crate::projects::ProjectInfo> {
    tauri::async_runtime::spawn_blocking(move || crate::projects::infos(&app))
        .await
        .unwrap_or_default()
}

/// Opens a project: editor and terminal. Only paths from the repo list.
#[tauri::command]
pub async fn project_launch(app: AppHandle, path: String) -> CmdResult<String> {
    let known = {
        let app = app.clone();
        let path = path.clone();
        tauri::async_runtime::spawn_blocking(move || {
            crate::projects::list(&app)
                .iter()
                .any(|p| p.to_string_lossy() == path)
        })
        .await
        .unwrap_or(false)
    };
    if !known {
        return Err("not a project in your code folders".into());
    }
    executor(&app.state::<AppState>())
        .run("launch_project", &serde_json::json!({ "path": path }))
        .await
        .map(|o| o.message)
        .map_err(|e| e.to_string())
}

/// Deletes everything in the search index. Folders are indexed
/// again on the next re-index.
#[tauri::command]
pub fn search_clear(state: State<'_, AppState>) -> CmdResult<usize> {
    lock(&state.storage)
        .clear_search(None)
        .map_err(|e| e.to_string())
}

const BACKUP_VERSION: u32 = 1;

/// Saves settings, your own skills and the action history to one file in
/// Documents and shows it. Composio headers are secrets, so they
/// are left out.
#[tauri::command]
pub async fn backup_export(app: AppHandle) -> CmdResult<String> {
    let path = write_backup(&app)?;
    let _ = executor(&app.state::<AppState>())
        .run("reveal_path", &serde_json::json!({ "path": path }))
        .await;
    Ok(path)
}

fn write_backup(app: &AppHandle) -> CmdResult<String> {
    let state = app.state::<AppState>();
    let mut settings = lock(&state.settings).clone();
    settings.composio.headers.clear();
    let mut skills = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&state.skills_dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "yaml" || x == "yml")
                && let Ok(yaml) = std::fs::read_to_string(&p)
            {
                skills.push(serde_json::json!({
                    "file": p.file_name().map(|n| n.to_string_lossy().into_owned()),
                    "yaml": yaml,
                }));
            }
        }
    }
    let history = lock(&state.storage)
        .recent_actions(5000)
        .map_err(|e| e.to_string())?;
    let bundle = serde_json::json!({
        "sidekickBackup": BACKUP_VERSION,
        "created": chrono::Utc::now().to_rfc3339(),
        "settings": settings,
        "skills": skills,
        "history": history,
    });
    let dir = dirs::document_dir().ok_or("no Documents folder")?;
    let path = dir.join(format!(
        "Sidekick backup {}.json",
        chrono::Local::now().format("%Y-%m-%d %H%M")
    ));
    let text = serde_json::to_string_pretty(&bundle).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
}

/// Restores settings and skills from a backup's text. Every skill is
/// checked first; the action history is kept for reference only.
#[tauri::command]
pub fn backup_import(app: AppHandle, text: String) -> CmdResult<String> {
    let bundle: serde_json::Value =
        serde_json::from_str(&text).map_err(|_| "This is not a Sidekick backup.".to_string())?;
    if bundle["sidekickBackup"].as_u64().is_none() {
        return Err("This is not a Sidekick backup.".into());
    }
    let state = app.state::<AppState>();
    let mut installed = 0;
    if let Some(skills) = bundle["skills"].as_array() {
        std::fs::create_dir_all(&state.skills_dir).map_err(|e| e.to_string())?;
        for s in skills {
            let Some(yaml) = s["yaml"].as_str() else {
                continue;
            };
            let Ok(skill) = sidekick_skills::Skill::parse("backup", yaml) else {
                continue;
            };
            let safe_id: String = skill
                .id
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
                .collect();
            if safe_id.is_empty() {
                continue;
            }
            if std::fs::write(state.skills_dir.join(format!("{safe_id}.yaml")), yaml).is_ok() {
                installed += 1;
            }
        }
        let (skills, errors) = sidekick_skills::load_all(&state.skills_dir);
        *lock(&state.engine) = sidekick_skills::Engine::new(skills);
        *lock(&state.skill_errors) = errors;
    }
    let mut restored = false;
    if let Ok(mut settings) = serde_json::from_value::<Settings>(bundle["settings"].clone()) {
        // Keep this PC's onboarding state.
        let current = lock(&state.settings).clone();
        settings.composio.headers = current.composio.headers;
        settings.onboarded = true;
        apply_settings(&app, settings)?;
        restored = true;
    }
    Ok(format!(
        "Restored {}{installed} skills",
        if restored { "settings and " } else { "" }
    ))
}
