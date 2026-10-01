use chrono::Utc;
use serde::Serialize;
use sidekick_core::{Event, MascotEvent, MascotState, Pause, Settings, StoredEvent};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_autostart::ManagerExt;

use crate::demo;
use crate::mascot;
use crate::pipeline::DEBUG_MANUAL_KIND;
use crate::state::{AppState, HitRect, Suggestion, gate_state, lock};
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
pub fn suggestion_current(state: State<'_, AppState>) -> Option<Suggestion> {
    lock(&state.suggestion).clone()
}

#[tauri::command]
pub fn suggestion_choose(app: AppHandle, id: String, index: usize) -> CmdResult<()> {
    demo::choose(&app, &id, index)
}

#[tauri::command]
pub fn suggestion_dismiss(app: AppHandle, id: String, reason: String) -> CmdResult<()> {
    demo::dismiss(&app, &id, &reason)
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
pub fn debug_demo_flow(app: AppHandle) -> CmdResult<()> {
    demo::start(&app)
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

    next.save(&state.settings_path).map_err(|e| e.to_string())?;
    *lock(&state.settings) = next.clone();
    state.gate.set(gate_state(&next, now));

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
