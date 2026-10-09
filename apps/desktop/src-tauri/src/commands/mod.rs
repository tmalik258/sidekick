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

mod chat;
mod cloud;
mod data;
mod diag;
mod notify;
mod setup;
mod voice;
pub use chat::*;
pub use cloud::*;
pub use data::*;
pub use diag::*;
pub use notify::*;
pub use setup::*;
pub use voice::*;

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
pub async fn app_info(app: AppHandle) -> CmdResult<AppInfo> {
    let counter = app.clone();
    let event_count = off_ui(move || {
        lock(&counter.state::<AppState>().storage)
            .count_events()
            .map_err(|e| e.to_string())
    })
    .await??;
    let state = app.state::<AppState>();
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

/// The island's page has drawn its first frame: show the window now.
#[tauri::command]
pub fn island_ready(app: AppHandle) {
    if let Some(w) = app.get_webview_window(crate::island::LABEL) {
        crate::island::reveal(&w);
    }
}

/// The update found by the last check, if any.
#[tauri::command]
pub fn update_status() -> Option<crate::updates::Available> {
    crate::updates::found()
}

/// Checks for a newer release now (Settings > Check now).
#[tauri::command]
pub async fn update_check(app: AppHandle) -> Result<Option<crate::updates::Available>, String> {
    crate::updates::check(&app).await
}

/// Downloads, verifies and runs the newest installer, then quits.
#[tauri::command]
pub async fn update_install(app: AppHandle) -> Result<String, String> {
    crate::updates::install(&app).await
}

/// Whether the internet is reachable, as last checked.
#[tauri::command]
pub fn net_status() -> bool {
    crate::net::online()
}

/// Checks the connection now. `lost` is set when Windows just reported the
/// network gone, so one failed try is enough to call it offline.
#[tauri::command]
pub async fn net_check(app: AppHandle, lost: bool) -> bool {
    crate::net::check(&app, lost).await
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
pub fn suggestion_choose(
    app: AppHandle,
    id: String,
    index: usize,
    private: Option<bool>,
) -> CmdResult<()> {
    if private == Some(true) {
        suggestions::make_private(&app, &id, index);
    }
    suggestions::choose(&app, &id, index)
}

#[tauri::command]
pub fn suggestion_dismiss(app: AppHandle, id: String, reason: String) -> CmdResult<()> {
    suggestions::dismiss(&app, &id, &reason)
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

/// Every loaded skill with the user's switches applied.
#[tauri::command]
pub async fn skills_list(app: AppHandle) -> Vec<SkillInfo> {
    off_ui(move || skills_now(&app.state::<AppState>()))
        .await
        .unwrap_or_default()
}

fn skills_now(state: &AppState) -> Vec<SkillInfo> {
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

/// Forgets which options the user picked before.
#[tauri::command]
pub fn choices_reset(app: AppHandle, state: State<'_, AppState>) -> CmdResult<usize> {
    decide::clear(&app);
    lock(&state.storage)
        .clear_choices()
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
    if next.ai != previous.ai {
        // Warm sessions were started with the old models and paths.
        crate::ai::close_sessions();
    }

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
        crate::start_features(app);
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
    } else if previous.hide_in_fullscreen != next.hide_in_fullscreen {
        // Switched from the glance: apply it to the window in front now.
        let front = lock(&state.last_window).clone().unwrap_or_default();
        island::follow_fullscreen(app, &front);
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
    if previous.voice != next.voice
        || previous.pause != next.pause
        || previous.onboarded != next.onboarded
        || previous.assistant_name != next.assistant_name
    {
        crate::voice::refresh(app);
    }

    if let Err(err) = app.emit(SETTINGS_CHANGED, &next) {
        log::warn!("could not emit settings change: {err}");
    }
    if next.code_editor != previous.code_editor {
        crate::editors::refresh(app);
    }
    Ok(next)
}

/// Installs a skill written in Ask mode, after checking it, and
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

/// Runs `f` on a worker thread. Commands that read the database or disk use
/// it: the storage lock can be held by a sensor write, and a sync command
/// would hold the UI thread while it waits (the e2e freeze check).
pub(crate) async fn off_ui<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())
}
