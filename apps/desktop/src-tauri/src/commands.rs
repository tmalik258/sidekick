use chrono::Utc;
use serde::Serialize;
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
    /// The skill runs on its own by default.
    auto_by_default: bool,
}

/// Every loaded skill with the user's switches applied (FR-SKL-06).
#[tauri::command]
pub fn skills_list(state: State<'_, AppState>) -> Vec<SkillInfo> {
    let settings = lock(&state.settings).clone();
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

    if previous.palette_hotkey != next.palette_hotkey
        && let Err(err) = ask::register(app, &next.palette_hotkey)
    {
        let _ = ask::register(app, &previous.palette_hotkey);
        return Err(err);
    }

    next.save(&state.settings_path).map_err(|e| e.to_string())?;
    *lock(&state.settings) = next.clone();
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
pub fn search(app: AppHandle, query: String) -> Vec<sidekick_core::SearchHit> {
    crate::search::search(&app, &query, &[], 30)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchStatus {
    items: u64,
}

#[tauri::command]
pub fn search_status(state: State<'_, AppState>) -> CmdResult<SearchStatus> {
    Ok(SearchStatus {
        items: lock(&state.storage)
            .search_count()
            .map_err(|e| e.to_string())?,
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
