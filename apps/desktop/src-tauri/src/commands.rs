use chrono::Utc;
use serde::{Deserialize, Serialize};
use sidekick_core::{
    ActionRecord, Event, MascotEvent, MascotState, Pause, Settings, SkillPref, StoredEvent,
};
use sidekick_skills::Trust;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_autostart::ManagerExt;

use crate::ai;
use crate::ask;
use crate::decide;
use crate::island;
use crate::mascot;
use crate::pipeline::DEBUG_MANUAL_KIND;
use crate::state::{AppState, HitRect, Suggestion, executor, gate_state, lock};
use crate::suggestions;
use crate::windows;

pub const SETTINGS_CHANGED: &str = "settings://changed";

type CmdResult<T> = Result<T, String>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    version: String,
    db_path: String,
    settings_path: String,
    event_count: u64,
}

#[tauri::command]
pub fn app_info(app: AppHandle, state: State<'_, AppState>) -> CmdResult<AppInfo> {
    let event_count = lock(&state.storage)
        .count_events()
        .map_err(|e| e.to_string())?;
    Ok(AppInfo {
        version: app.package_info().version.to_string(),
        db_path: state.db_path.display().to_string(),
        settings_path: state.settings_path.display().to_string(),
        event_count,
    })
}

#[tauri::command]
pub fn settings_get(state: State<'_, AppState>) -> Settings {
    lock(&state.settings).clone()
}

#[tauri::command]
pub fn settings_set(app: AppHandle, settings: Settings) -> CmdResult<Settings> {
    apply_settings(&app, settings)
}

/// Pauses sensors for `minutes`, or until resumed when `minutes` is null.
#[tauri::command]
pub fn sensors_pause(app: AppHandle, minutes: Option<u32>) -> CmdResult<Settings> {
    let pause = match minutes {
        Some(m) => Pause::for_minutes(m, Utc::now()),
        None => Pause::Indefinite,
    };
    set_pause(&app, pause)
}

#[tauri::command]
pub fn sensors_resume(app: AppHandle) -> CmdResult<Settings> {
    set_pause(&app, Pause::None)
}

#[tauri::command]
pub fn mascot_get(app: AppHandle) -> MascotState {
    mascot::current(&app)
}

#[tauri::command]
pub fn island_set_hit_rect(state: State<'_, AppState>, rect: HitRect) {
    *lock(&state.hit_rect) = rect;
}

#[tauri::command]
pub fn suggestion_current(app: AppHandle) -> Option<Suggestion> {
    suggestions::current(&app)
}

#[tauri::command]
pub fn suggestion_choose(app: AppHandle, id: String, index: usize) -> CmdResult<()> {
    suggestions::choose(&app, &id, index)
}

#[tauri::command]
pub fn suggestion_dismiss(app: AppHandle, id: String, reason: String) -> CmdResult<()> {
    suggestions::dismiss(&app, &id, &reason)
}

#[tauri::command]
pub fn events_recent(
    state: State<'_, AppState>,
    limit: Option<u32>,
) -> CmdResult<Vec<StoredEvent>> {
    lock(&state.storage)
        .recent_events(limit.unwrap_or(50).min(500))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn open_settings(app: AppHandle) -> CmdResult<()> {
    windows::open_settings(&app).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn debug_set_state(app: AppHandle, state: MascotState) {
    mascot::force(&app, state);
}

#[tauri::command]
pub fn debug_emit_event(state: State<'_, AppState>) {
    state.bus.publish(Event::new(
        DEBUG_MANUAL_KIND,
        "debug",
        serde_json::json!({ "from": "settings" }),
    ));
}

#[tauri::command]
pub fn debug_demo_flow(app: AppHandle) {
    suggestions::demo(&app);
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillInfo {
    id: String,
    name: String,
    description: String,
    event: String,
    enabled: bool,
    auto: bool,
    /// Quiet after "Not now" three times in a row, until this time.
    muted_until: Option<String>,
    /// The skill runs on its own by default.
    auto_by_default: bool,
}

/// Every loaded skill with the user's switches applied (FR-SKL-06).
#[tauri::command]
pub fn skills_list(state: State<'_, AppState>) -> Vec<SkillInfo> {
    let settings = lock(&state.settings).clone();
    let now = chrono::Utc::now();
    let muted = |id: &str| {
        lock(&state.storage)
            .habit(id)
            .ok()
            .and_then(|h| h.muted_until)
            .filter(|t| chrono::DateTime::parse_from_rfc3339(t).is_ok_and(|t| t > now))
    };
    lock(&state.engine)
        .skills()
        .iter()
        .map(|s| {
            let pref = settings.skills.get(&s.id).cloned().unwrap_or_default();
            SkillInfo {
                id: s.id.clone(),
                name: s.name.clone(),
                description: s.description.clone(),
                event: s.trigger.event.clone(),
                enabled: pref.enabled.unwrap_or(s.enabled_by_default),
                auto: pref.auto.unwrap_or(s.trust == Trust::Auto),
                muted_until: muted(&s.id),
                auto_by_default: s.trust == Trust::Auto,
            }
        })
        .collect()
}

#[tauri::command]
pub fn skill_set(app: AppHandle, id: String, enabled: bool, auto: bool) -> CmdResult<Settings> {
    let mut settings = lock(&app.state::<AppState>().settings).clone();
    settings.skills.insert(
        id,
        SkillPref {
            enabled: Some(enabled),
            auto: Some(auto),
        },
    );
    apply_settings(&app, settings)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityInfo {
    found: Vec<String>,
    skills_dir: String,
    skill_errors: Vec<String>,
}

/// What browsers and tools Sidekick found, optionally scanning again.
#[tauri::command]
pub async fn capabilities_get(app: AppHandle, rescan: bool) -> CmdResult<CapabilityInfo> {
    let state = app.state::<AppState>();
    if rescan {
        let caps = tauri::async_runtime::spawn_blocking(sidekick_actions::Capabilities::detect)
            .await
            .map_err(|e| e.to_string())?;
        *state
            .executor
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            std::sync::Arc::new(sidekick_actions::Executor::new(caps));
    }
    let exec = executor(&state);
    Ok(CapabilityInfo {
        found: exec.capabilities().summary(),
        skills_dir: state.skills_dir.display().to_string(),
        skill_errors: lock(&state.skill_errors).clone(),
    })
}

/// Forgets which options the user picked before (FR-DEV-02).
#[tauri::command]
pub fn choices_reset(app: AppHandle, state: State<'_, AppState>) -> CmdResult<usize> {
    decide::clear(&app);
    lock(&state.storage)
        .clear_choices()
        .map_err(|e| e.to_string())
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

/// Shows a file an action produced. Only existing paths, nothing else runs.
#[tauri::command]
pub async fn reveal_path(app: AppHandle, path: String) -> CmdResult<()> {
    let exec = executor(&app.state::<AppState>());
    exec.run("reveal_path", &serde_json::json!({ "path": path }))
        .await
        .map(drop)
        .map_err(|e| e.to_string())
}

pub fn set_pause(app: &AppHandle, pause: Pause) -> CmdResult<Settings> {
    let mut settings = lock(&app.state::<AppState>().settings).clone();
    settings.pause = pause;
    apply_settings(app, settings)
}

/// The single path for changing settings: sanitize, apply side effects,
/// persist, update the sensor gate, and notify every window.
pub fn apply_settings(app: &AppHandle, next: Settings) -> CmdResult<Settings> {
    let state = app.state::<AppState>();
    let next = next.sanitized();
    let now = Utc::now();
    let previous = lock(&state.settings).clone();

    if next.launch_at_login != previous.launch_at_login {
        let autolaunch = app.autolaunch();
        let result = if next.launch_at_login {
            autolaunch.enable()
        } else {
            autolaunch.disable()
        };
        result.map_err(|e| format!("could not change launch at login: {e}"))?;
    }

    if let Some(bad) = next
        .shortcuts
        .values()
        .find(|k| !crate::shortcuts::valid(k))
    {
        return Err(format!("{bad} is not a valid shortcut"));
    }
    let keys_changed =
        previous.palette_hotkey != next.palette_hotkey || previous.shortcuts != next.shortcuts;

    next.save(&state.settings_path).map_err(|e| e.to_string())?;
    *lock(&state.settings) = next.clone();
    if next.onboarded && !previous.onboarded {
        ask::release_sticky();
    }
    if keys_changed && let Err(err) = ask::register(app, &next.palette_hotkey) {
        // Put the old keys back so Ask keeps working.
        previous
            .save(&state.settings_path)
            .map_err(|e| e.to_string())?;
        *lock(&state.settings) = previous.clone();
        let _ = ask::register(app, &previous.palette_hotkey);
        return Err(err);
    }
    state.gate.set(gate_state(&next, now));
    if previous.ai != next.ai {
        let app = app.clone();
        tauri::async_runtime::spawn(async move { ai::refresh_readiness(&app).await });
    }
    // Without the window sensor nothing would bring a hidden island back.
    if next.pause.is_active(now) || !next.sensor_enabled("window") {
        island::follow_fullscreen(app, &serde_json::Value::Null);
    }

    match (previous.pause.is_active(now), next.pause.is_active(now)) {
        (false, true) => {
            mascot::dispatch(app, MascotEvent::Rest);
        }
        (true, false) => {
            mascot::dispatch(app, MascotEvent::Wake);
        }
        _ => {}
    }

    if previous.calendar != next.calendar {
        sync_calendar(&state.calendar, &next);
    }
    if previous.voice != next.voice || previous.pause != next.pause {
        crate::voice::refresh(app);
    }

    if let Err(err) = app.emit(SETTINGS_CHANGED, &next) {
        log::warn!("could not emit settings change: {err}");
    }
    Ok(next)
}

#[tauri::command]
pub async fn ai_status(app: AppHandle) -> Vec<ai::ProviderStatus> {
    ai::status(&app).await
}

#[tauri::command]
pub fn ai_chat(
    app: AppHandle,
    id: String,
    messages: Vec<sidekick_ai::Message>,
    attach: ai::Attach,
    local_only: bool,
) {
    ai::chat(&app, id, messages, attach, local_only);
}

#[tauri::command]
pub fn ai_cancel(app: AppHandle, id: String) {
    ai::cancel(&app, &id);
}

/// Turns the island into Ask mode, optionally with a prompt.
#[tauri::command]
pub fn ask_open(app: AppHandle, prompt: Option<String>, ask: bool) {
    ask::open(
        &app,
        ask::Open {
            prompt,
            ask,
            ..Default::default()
        },
    );
}

/// Shows welcome when the island is ready and onboarding is not done yet.
#[tauri::command]
pub fn ask_ensure_welcome(app: AppHandle) {
    ask::ensure_welcome(&app);
}

/// Parks welcome until the user hovers the island again (does not finish onboarding).
#[tauri::command]
pub fn ask_defer_welcome(app: AppHandle) {
    ask::defer_welcome(&app);
}

#[tauri::command]
pub fn ask_close(app: AppHandle) {
    ask::close(&app);
}

/// Moves what an earlier action created to the Recycle Bin.
#[tauri::command]
pub fn action_undo(app: AppHandle, id: i64) -> CmdResult<String> {
    crate::undo::undo(&app, id)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserInfo {
    token: String,
    port: u16,
}

/// The pairing code to paste into the browser extension.
#[tauri::command]
pub fn browser_info(state: State<'_, AppState>) -> BrowserInfo {
    BrowserInfo {
        token: state.browser_token.clone(),
        port: sidekick_sensors::BrowserSensor::DEFAULT_PORT,
    }
}

/// Today's time per app and project, largest first.
#[tauri::command]
pub fn time_today(state: State<'_, AppState>) -> CmdResult<Vec<sidekick_core::AppTime>> {
    let day = chrono::Local::now().format("%Y-%m-%d").to_string();
    lock(&state.storage)
        .time_for_day(&day)
        .map_err(|e| e.to_string())
}

/// Installs a skill written in Ask mode, after checking it (FR-SKL-08), and
/// reloads all skills. Returns the skill's name.
#[tauri::command]
pub fn skill_install(state: State<'_, AppState>, yaml: String) -> CmdResult<String> {
    let skill = sidekick_skills::validate_new(&yaml)?;
    std::fs::create_dir_all(&state.skills_dir).map_err(|e| e.to_string())?;
    let path = state.skills_dir.join(format!("{}.yaml", skill.id));
    std::fs::write(&path, yaml).map_err(|e| format!("could not save the skill: {e}"))?;
    let (skills, errors) = sidekick_skills::load_all(&state.skills_dir);
    *lock(&state.engine) = sidekick_skills::Engine::new(skills);
    *lock(&state.skill_errors) = errors;
    log::info!("installed skill {} at {}", skill.id, path.display());
    Ok(skill.name)
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpInfo {
    url: String,
    token: String,
}

/// How to add Sidekick to Claude Code as an MCP server.
#[tauri::command]
pub fn mcp_info(state: State<'_, AppState>) -> McpInfo {
    McpInfo {
        url: format!("http://127.0.0.1:{}/mcp", crate::mcp::PORT),
        token: state.mcp_token.clone(),
    }
}

#[tauri::command]
pub fn voice_status(app: AppHandle) -> crate::voice::VoiceStatus {
    crate::voice::status(&app)
}

#[tauri::command]
pub fn voice_download(app: AppHandle) -> CmdResult<()> {
    crate::voice::download(&app)
}

#[tauri::command]
pub fn voice_cancel_download(app: AppHandle) {
    crate::voice::cancel_download(&app);
}

#[tauri::command]
pub fn voice_listen(app: AppHandle) -> CmdResult<()> {
    crate::voice::listen(&app)
}

#[tauri::command]
pub fn voice_stop(app: AppHandle) {
    crate::voice::stop(&app);
}

/// The welcome line and when each part of it is heard.
#[tauri::command]
pub fn voice_welcome(app: AppHandle) -> crate::voice::WelcomeSpeech {
    crate::voice::welcome_speech(&app)
}

#[tauri::command]
pub fn voice_test(app: AppHandle) -> CmdResult<()> {
    crate::voice::test(&app)
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

/// Recent clipboard text, newest first (FR-CLIP-01). Secrets are never in it.
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

/// Repos in the code folders, for the project launcher (FR-DEV-10).
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

/// Deletes everything in the search index (FR-RAG-12). Folders are indexed
/// again on the next re-index.
#[tauri::command]
pub fn search_clear(state: State<'_, AppState>) -> CmdResult<usize> {
    lock(&state.storage)
        .clear_search(None)
        .map_err(|e| e.to_string())
}

const BACKUP_VERSION: u32 = 1;

/// Saves settings, your own skills and the action history to one file in
/// Documents and shows it (FR-SET-04). Composio headers are secrets, so they
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupStatus {
    items: Vec<crate::setup::SetupItem>,
    /// Installs every missing recommended tool in one go.
    install_all: Option<String>,
}

/// The setup checklist, checked again each time.
#[tauri::command]
pub async fn setup_status(app: AppHandle) -> SetupStatus {
    let items = crate::setup::status(&app).await;
    let install_all = crate::setup::install_all(&items);
    SetupStatus { items, install_all }
}

/// Opens a PowerShell window running one setup step ("all" for every
/// missing recommended tool).
#[tauri::command]
pub async fn setup_run(app: AppHandle, id: String) -> CmdResult<()> {
    crate::setup::run(&app, &id).await
}

/// Opens Claude Code in a terminal with this conversation, to finish what
/// the local model could not.
#[tauri::command]
pub async fn ai_handoff(
    app: AppHandle,
    messages: Vec<sidekick_ai::Message>,
    reason: Option<String>,
) -> CmdResult<String> {
    crate::composio::open_in_claude_code(&app, &messages, reason.as_deref())
        .await
        .map(|dir| dir.display().to_string())
}

/// Opens Composio in the browser to sign in; returns the code it shows.
#[tauri::command]
pub async fn composio_sign_in(app: AppHandle) -> CmdResult<String> {
    crate::composio::sign_in(&app).await
}

#[tauri::command]
pub fn composio_sign_out(app: AppHandle) -> CmdResult<()> {
    crate::composio::sign_out(&app)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposioStatus {
    signed_in: bool,
    account: String,
    apps: Vec<crate::composio_api::App>,
    error: Option<String>,
}

/// Whether Composio is connected, and which of Sidekick's apps are.
#[tauri::command]
pub async fn composio_status(app: AppHandle) -> ComposioStatus {
    let c = lock(&app.state::<AppState>().settings).composio.clone();
    if !crate::composio::signed_in() {
        return ComposioStatus {
            signed_in: false,
            account: String::new(),
            apps: Vec::new(),
            error: None,
        };
    }
    let (apps, error) = match crate::composio::apps(&c).await {
        Ok(a) => (a, None),
        Err(e) => (Vec::new(), Some(e)),
    };
    ComposioStatus {
        signed_in: true,
        account: c.account,
        apps,
        error,
    }
}

/// Opens the browser to connect one app on Composio.
#[tauri::command]
pub async fn composio_connect(app: AppHandle, slug: String) -> CmdResult<()> {
    crate::composio::connect_app(&app, &slug).await
}

/// Copies the Composio server from Claude Code's config into Settings.
#[tauri::command]
pub fn composio_import(app: AppHandle) -> CmdResult<Settings> {
    let home = dirs::home_dir().ok_or("no home folder")?;
    let text = std::fs::read_to_string(home.join(".claude.json")).unwrap_or_default();
    let (url, headers) = crate::composio::from_claude_config(&text)
        .ok_or("No Composio server in Claude Code's settings (~/.claude.json)")?;
    let mut settings = lock(&app.state::<AppState>().settings).clone();
    settings.composio.url = url;
    settings.composio.headers = headers;
    settings.composio.enabled = true;
    apply_settings(&app, settings)
}

/// Connects to Composio and counts its tools.
#[tauri::command]
pub async fn composio_test(app: AppHandle) -> CmdResult<crate::composio::Check> {
    let settings = lock(&app.state::<AppState>().settings).composio.clone();
    crate::composio::test(&settings).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Found {
    code_folders: Vec<crate::detect::Folder>,
    search_folders: Vec<crate::detect::Folder>,
    chat_models: Vec<String>,
    embed_models: Vec<String>,
    claude_installed: bool,
    claude_hooks: bool,
    claude_mcp: bool,
    composio_signed_in: bool,
    composio_in_claude: bool,
    browsers: Vec<String>,
    /// Setup steps that can run in one go, missing now.
    installable: Vec<crate::setup::SetupItem>,
}

/// Everything Sidekick can set up on its own, found on this PC.
#[tauri::command]
pub async fn setup_detect(app: AppHandle) -> Found {
    let state = app.state::<AppState>();
    let settings = lock(&state.settings).clone();
    let items = crate::setup::status(&app).await;
    let done = |id: &str| items.iter().any(|i| i.id == id && i.done);
    let models = crate::setup::ollama_models(&settings.ai.local.base_url)
        .await
        .unwrap_or_default();
    let (embed, chat): (Vec<String>, Vec<String>) = models
        .into_iter()
        .partition(|m| sidekick_ai::is_embedding_model(m));
    let home = dirs::home_dir().unwrap_or_default();
    let claude_json = std::fs::read_to_string(home.join(".claude.json")).unwrap_or_default();
    let (code_folders, search_folders) = tauri::async_runtime::spawn_blocking(|| {
        (crate::detect::code_roots(), crate::detect::search_folders())
    })
    .await
    .unwrap_or_default();
    Found {
        code_folders,
        search_folders,
        chat_models: chat,
        embed_models: embed,
        claude_installed: done("claude_code"),
        claude_hooks: done("claude_hooks"),
        claude_mcp: done("claude_mcp"),
        composio_signed_in: crate::composio::signed_in(),
        composio_in_claude: crate::composio::from_claude_config(&claude_json).is_some(),
        browsers: executor(&state)
            .capabilities()
            .browsers
            .iter()
            .map(|b| b.id.clone())
            .collect(),
        installable: items
            .into_iter()
            .filter(|i| i.runnable && !i.done && i.recommended)
            .collect(),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    code_folders: Vec<String>,
    search_folders: Vec<String>,
    chat_model: Option<String>,
    claude_hooks: bool,
    claude_mcp: bool,
    /// Setup step ids to run in one PowerShell window.
    install: Vec<String>,
    voice: bool,
    launch_at_login: bool,
}

/// Applies the choices from the welcome screen. Returns what was done.
#[tauri::command]
pub async fn setup_apply(app: AppHandle, plan: Plan) -> CmdResult<Vec<String>> {
    let mut done = Vec::new();
    let mut settings = lock(&app.state::<AppState>().settings).clone();
    if !plan.code_folders.is_empty() {
        settings.code_folders = plan.code_folders;
        done.push("Code folders set".to_owned());
    }
    if !plan.search_folders.is_empty() {
        settings.index_folders = plan.search_folders;
        done.push("Search folders set".to_owned());
    }
    if let Some(m) = plan.chat_model.filter(|m| !m.trim().is_empty()) {
        settings.ai.local.model = m;
        settings.ai.local.enabled = true;
    }
    settings.voice.enabled |= plan.voice;
    settings.launch_at_login = plan.launch_at_login;
    apply_settings(&app, settings)?;
    if plan.voice {
        let _ = crate::voice::download(&app);
    }
    if plan.claude_hooks {
        crate::claude_config::add_hooks()?;
        done.push("Claude Code hooks added".to_owned());
    }
    if plan.claude_mcp {
        claude_add_mcp(app.clone()).await?;
        done.push("Sidekick tools added to Claude Code".to_owned());
    }
    if !plan.install.is_empty() {
        crate::setup::run_many(&app, &plan.install).await?;
        done.push("Installing in PowerShell".to_owned());
    }
    crate::search::reindex_folders(&app);
    Ok(done)
}

/// Adds Sidekick's hooks to Claude Code's settings (backed up first).
#[tauri::command]
pub fn claude_add_hooks() -> CmdResult<Option<String>> {
    crate::claude_config::add_hooks().map(|b| b.map(|p| p.display().to_string()))
}

/// Adds Sidekick's MCP server to Claude Code with `claude mcp add`.
#[tauri::command]
pub async fn claude_add_mcp(app: AppHandle) -> CmdResult<()> {
    let state = app.state::<AppState>();
    let path = lock(&state.settings).ai.claude_code.path.trim().to_owned();
    let claude = if path.is_empty() {
        which::which("claude").map_err(|_| "Install Claude Code first".to_string())?
    } else {
        std::path::PathBuf::from(path)
    };
    let url = format!("http://127.0.0.1:{}/mcp", crate::mcp::PORT);
    crate::claude_config::add_mcp(&claude, &url, &state.mcp_token).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserStatus {
    id: String,
    name: String,
    /// The extension checked in from this kind of browser recently.
    connected: bool,
}

/// Installed browsers and whether the extension is connected in each.
#[tauri::command]
pub fn browsers_status(app: AppHandle) -> Vec<BrowserStatus> {
    let state = app.state::<AppState>();
    let seen = state.browser.seen();
    let now = chrono::Utc::now().timestamp();
    let recent = |name: &str| {
        seen.iter()
            .any(|(n, t)| n.eq_ignore_ascii_case(name) && now - t < 7 * 24 * 3600)
    };
    executor(&state)
        .capabilities()
        .browsers
        .iter()
        .map(|b| {
            let name = b.label().to_owned();
            // Firefox and Zen report as Firefox.
            let reported = if b.id == "zen" {
                "Firefox"
            } else {
                name.as_str()
            };
            BrowserStatus {
                connected: recent(reported),
                id: b.id.clone(),
                name,
            }
        })
        .collect()
}

/// Opens a browser's extensions page with the extension's path copied.
#[tauri::command]
pub async fn extension_install(
    app: AppHandle,
    browser: String,
) -> CmdResult<crate::extension::Guide> {
    crate::extension::install(&app, &browser).await
}

/// Runs an option and makes its skill automatic ("Always do this").
#[tauri::command]
pub fn suggestion_always(app: AppHandle, id: String, index: usize) -> CmdResult<()> {
    suggestions::always(&app, &id, index)
}

#[tauri::command]
pub fn later_list(app: AppHandle) -> Vec<suggestions::LaterItem> {
    suggestions::later_list(&app)
}

#[tauri::command]
pub fn later_open(app: AppHandle, id: String) -> CmdResult<()> {
    suggestions::later_open(&app, &id)
}

#[tauri::command]
pub fn later_clear(app: AppHandle) {
    suggestions::later_clear(&app);
}

/// Lets a skill quieted by "Not now" speak again.
#[tauri::command]
pub fn skill_unmute(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    let storage = lock(&state.storage);
    let Ok(mut h) = storage.habit(&id) else {
        return Ok(());
    };
    h.muted_until = None;
    h.dismiss_streak = 0;
    storage.save_habit(&h).map_err(|e| e.to_string())
}

/// Recent Ask conversations, newest first.
#[tauri::command]
pub fn chats_list(state: State<'_, AppState>) -> CmdResult<Vec<sidekick_core::ChatSummary>> {
    lock(&state.storage)
        .recent_chats(30)
        .map_err(|e| e.to_string())
}

/// A saved conversation's turns, as the UI saved them.
#[tauri::command]
pub fn chat_get(state: State<'_, AppState>, id: String) -> CmdResult<serde_json::Value> {
    let text = lock(&state.storage)
        .chat_turns(&id)
        .map_err(|e| e.to_string())?
        .ok_or("That conversation is gone")?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn chat_save(
    state: State<'_, AppState>,
    id: String,
    title: String,
    turns: serde_json::Value,
) -> CmdResult<()> {
    let title: String = title.trim().chars().take(80).collect();
    lock(&state.storage)
        .save_chat(&id, &title, &turns.to_string())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn chat_delete(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    lock(&state.storage)
        .delete_chat(&id)
        .map_err(|e| e.to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalModels {
    reachable: bool,
    chat: Vec<String>,
    embed: Vec<String>,
}

/// Models on the local AI server, split into chat and search models.
#[tauri::command]
pub async fn local_models(app: AppHandle) -> LocalModels {
    let base = lock(&app.state::<AppState>().settings)
        .ai
        .local
        .base_url
        .clone();
    let Some(models) = crate::setup::ollama_models(&base).await else {
        return LocalModels {
            reachable: false,
            chat: Vec::new(),
            embed: Vec::new(),
        };
    };
    let (embed, chat) = models
        .into_iter()
        .partition(|m| sidekick_ai::is_embedding_model(m));
    LocalModels {
        reachable: true,
        chat,
        embed,
    }
}

/// Programs running now with a window, for the ignore list picker.
#[tauri::command]
pub async fn running_apps() -> Vec<String> {
    tauri::async_runtime::spawn_blocking(|| {
        let mut sys = sysinfo::System::new();
        sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
        let mut names: Vec<String> = sys
            .processes()
            .values()
            .filter_map(|p| p.name().to_str().map(str::to_ascii_lowercase))
            .filter(|n| n.ends_with(".exe") && !SYSTEM_EXES.contains(&n.as_str()))
            .collect();
        names.sort();
        names.dedup();
        names
    })
    .await
    .unwrap_or_default()
}

/// Windows' own processes; never worth ignoring.
const SYSTEM_EXES: &[&str] = &[
    "svchost.exe",
    "csrss.exe",
    "wininit.exe",
    "winlogon.exe",
    "services.exe",
    "lsass.exe",
    "smss.exe",
    "dwm.exe",
    "fontdrvhost.exe",
    "conhost.exe",
    "runtimebroker.exe",
    "sihost.exe",
    "taskhostw.exe",
    "ctfmon.exe",
    "searchindexer.exe",
    "spoolsv.exe",
    "audiodg.exe",
    "dllhost.exe",
    "registry",
    "system",
    "memory compression",
];
