mod act;
mod agents;
mod ai;
mod ask;
mod ask_tools;
mod brief;
mod browser;
mod chat_prune;
mod claude_config;
mod codex_config;
mod commands;
mod composio;
mod composio_api;
mod decide;
#[cfg(test)]
mod decisions_test;
mod detect;
#[cfg(test)]
mod drift_test;
mod extension;
mod fathom;
mod files;
mod find;
mod health;
mod inbox;
mod island;
mod layout;
mod learn;
mod mascot;
mod mcp;
mod mcp_oauth;
mod meetings;
mod moments;
mod net;
mod office;
mod pipeline;
mod privacy;
mod projects;
mod recipes;
mod routines;
mod screen;
mod search;
mod secrets;
mod setup;
mod shortcuts;
mod state;
mod stuck;
mod suggestions;
mod timetrack;
mod tray;
mod undo;
mod updates;
mod voice;
mod web;
mod windows;

use std::error::Error;
use std::sync::{Arc, Mutex, RwLock};

use chrono::Utc;
use sidekick_actions::{Capabilities, Executor};
use sidekick_core::{EventBus, MascotEvent, Settings, Storage};
use sidekick_sensors::{
    BrowserBridge, BrowserSensor, ClaudeCodeSensor, ClipboardSensor, DownloadsSensor, IdleSensor,
    PortsSensor, ReposSensor, Sensor, SensorGate, SystemSensor, WindowSensor,
};
use sidekick_skills::Engine;
use tauri::{AppHandle, Manager};

use crate::state::AppState;

/// Release builds stop on a panic, so where it happened is written to a
/// file first (and the log), for the next bug report.
fn log_panics(file: std::path::PathBuf) {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let note = format!("{} {info}\n", Utc::now().to_rfc3339());
        let _ = std::fs::write(&file, &note);
        log::error!("panic: {info}");
        default(info);
    }));
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Err(err) = windows::open_settings(app) {
                log::warn!("could not open settings on second launch: {err}");
            }
        }))
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .max_file_size(10 * 1024 * 1024)
                .build(),
        )
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(|app| {
            setup(app.handle())?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::settings_get,
            commands::settings_set,
            commands::sensors_pause,
            commands::sensors_resume,
            commands::mascot_get,
            commands::island_set_hit_rect,
            commands::island_ready,
            commands::net_status,
            commands::update_status,
            commands::update_check,
            commands::update_install,
            commands::net_check,
            commands::suggestion_current,
            commands::suggestion_choose,
            commands::suggestion_dismiss,
            commands::events_recent,
            commands::open_settings,
            commands::calendar_today,
            commands::search_clear,
            commands::setup_status,
            commands::setup_detect,
            commands::local_models,
            commands::running_apps,
            commands::chats_list,
            commands::chat_get,
            commands::chat_save,
            commands::chat_delete,
            commands::suggestion_always,
            commands::later_list,
            commands::later_open,
            commands::later_clear,
            commands::skill_unmute,
            commands::browsers_status,
            commands::extension_install,
            commands::setup_apply,
            commands::claude_add_hooks,
            commands::claude_add_mcp,
            commands::ai_handoff,
            commands::ai_run_proposal,
            commands::composio_import,
            commands::composio_sign_in,
            commands::composio_sign_out,
            commands::composio_status,
            commands::composio_connect,
            commands::composio_use_key,
            commands::agents_status,
            commands::ai_open_link,
            commands::codex_add_notify,
            commands::codex_add_mcp,
            commands::guide_keys,
            commands::recipe_save,
            commands::recipe_delete,
            commands::recipe_run,
            commands::know_how_clear,
            commands::notifications_status,
            commands::notifications_set_level,
            commands::dnd_get,
            commands::dnd_set,
            commands::setup_run,
            commands::backup_export,
            commands::backup_import,
            commands::projects_list,
            commands::project_launch,
            commands::clipboard_history,
            commands::clipboard_copy,
            commands::voice_status,
            commands::voice_download,
            commands::voice_cancel_download,
            commands::voice_listen,
            commands::voice_stop,
            commands::voice_test,
            commands::voice_welcome,
            commands::voice_welcome_step,
            commands::voice_say,
            commands::debug_set_state,
            commands::debug_emit_event,
            commands::debug_demo_flow,
            commands::skills_list,
            commands::skill_set,
            commands::capabilities_get,
            commands::choices_reset,
            commands::routines_today,
            commands::routines_forget,
            commands::actions_recent,
            commands::reveal_path,
            commands::ai_status,
            commands::ai_chat,
            commands::ai_cancel,
            commands::ask_open,
            commands::ask_ensure_welcome,
            commands::ask_defer_welcome,
            commands::ask_resume_welcome,
            commands::ask_close,
            commands::browser_info,
            commands::time_today,
            commands::skill_install,
            commands::search,
            commands::search_status,
            commands::search_reindex,
            commands::open_reference,
            commands::action_undo,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Sidekick");
}

fn setup(app: &AppHandle) -> Result<(), Box<dyn Error>> {
    let product_name = app
        .config()
        .product_name
        .as_deref()
        .ok_or("productName is missing from tauri.conf.json")?;
    secrets::init(product_name);
    let config_dir = app.path().app_config_dir()?;
    let settings_path = config_dir.join("settings.json");
    let skills_dir = config_dir.join("skills");
    let _ = std::fs::create_dir_all(&skills_dir);
    let (skills, skill_errors) = sidekick_skills::load_all(&skills_dir);
    for err in &skill_errors {
        log::warn!("skill not loaded: {err}");
    }
    log::info!("{} skills loaded", skills.len());
    let caps = Capabilities::detect();
    log::info!("found: {}", caps.summary().join(", "));
    let data_dir = app.path().app_data_dir()?;
    log_panics(data_dir.join("last-crash.txt"));
    let db_path = data_dir.join("sidekick.db");
    let scratch_dir = app.path().app_cache_dir()?;

    let settings = Settings::load(&settings_path);
    let storage = Storage::open(&db_path)?;
    let bus = EventBus::default();
    let (gate_handle, gate) = SensorGate::new(state::gate_state(&settings, Utc::now()));
    let paused = settings.pause.is_active(Utc::now());
    let hotkey = settings.palette_hotkey.clone();
    let repos = ReposSensor {
        roots: repo_roots(&settings),
        hour: settings.end_of_day_hour,
    };
    let browser_token = browser::load_or_create_token(&data_dir);
    let mcp_token = browser::load_or_create_secret(&data_dir, "mcp-token");
    let bridge = BrowserBridge::default();
    let calendar = sidekick_sensors::Calendar::default();
    let approvals = sidekick_sensors::Approvals::default();
    commands::sync_calendar(&calendar, &settings);

    app.manage(AppState {
        settings: Mutex::new(settings),
        settings_path,
        storage: Arc::new(Mutex::new(storage)),
        db_path,
        skills_dir,
        bus: bus.clone(),
        gate: gate_handle,
        mascot: Mutex::default(),
        mascot_epoch: Default::default(),
        hit_rect: Mutex::default(),
        engine: Mutex::new(Engine::new(skills)),
        skill_errors: Mutex::new(skill_errors),
        executor: RwLock::new(Arc::new(Executor::new(caps))),
        active: Mutex::default(),
        queue: Mutex::default(),
        later: Mutex::default(),
        island_hidden: Mutex::default(),
        hovered: Default::default(),
        last_window: Mutex::default(),
        chats: Mutex::default(),
        ask_proposals: Mutex::new(ask_tools::load_proposals(&data_dir)),
        ai_workdir: data_dir.join("claude-workspace"),
        voice: voice::Voice::new(data_dir.join("voice-models")),
        calendar: calendar.clone(),
        approvals: approvals.clone(),
        scratch_dir,
        decisions: Mutex::default(),
        ai_ready: Default::default(),
        away: Default::default(),
        linger: Default::default(),
        own_files: Mutex::default(),
        ask_open: Default::default(),
        tracker: Default::default(),
        browser: bridge.clone(),
        browser_token: browser_token.clone(),
        mcp_token,
        data_dir,
        features_started: Default::default(),
    });

    pipeline::start(app);
    // Prefer bundled models; only then network. Welcome opens from the island
    // once it listens (ask_ensure_welcome), or after models become ready.
    voice::seed_from_bundle(app);
    voice::refresh(app);
    voice::start_echo_guard(app);
    if !settings_onboarded(app) && !voice::voice_ready(app) {
        voice::prepare_then_welcome(app);
    }
    if settings_onboarded(app) {
        start_features(app);
    }
    net::start(app);
    tauri::async_runtime::spawn(async move {
        let sensors: Vec<Box<dyn Sensor>> = vec![
            Box::new(DownloadsSensor::new()),
            Box::new(DownloadsSensor::screenshots()),
            Box::new(PortsSensor),
            Box::new(ClipboardSensor),
            Box::new(WindowSensor),
            Box::new(ClaudeCodeSensor {
                port: ClaudeCodeSensor::DEFAULT_PORT,
                approvals: approvals.clone(),
            }),
            Box::new(BrowserSensor {
                port: BrowserSensor::DEFAULT_PORT,
                token: browser_token,
                bridge,
                approvals,
            }),
            Box::new(SystemSensor),
            Box::new(sidekick_sensors::CalendarSensor { state: calendar }),
            Box::new(repos),
            Box::new(IdleSensor::default()),
        ];
        sidekick_sensors::spawn_all(sensors, &bus, &gate);
    });

    island::setup(app)?;
    ask::setup(app, &hotkey);
    ai::watch_readiness(app);
    tray::create(app)?;

    if paused {
        mascot::dispatch(app, MascotEvent::Rest);
    }
    log::info!("Sidekick started");
    Ok(())
}

fn settings_onboarded(app: &tauri::AppHandle) -> bool {
    state::lock(&app.state::<AppState>().settings).onboarded
}

fn repo_roots(settings: &Settings) -> Vec<std::path::PathBuf> {
    if settings.code_folders.is_empty() {
        ReposSensor::default_roots()
    } else {
        settings.code_folders.iter().map(Into::into).collect()
    }
}

/// Starts everything Sidekick does on its own: briefs, moments, search,
/// updates and the rest. Runs once, at startup when onboarding is already
/// done, or the moment it finishes; until then Sidekick only onboards.
pub fn start_features(app: &AppHandle) {
    let state = app.state::<AppState>();
    if state
        .features_started
        .swap(true, std::sync::atomic::Ordering::SeqCst)
    {
        return;
    }
    let roots = repo_roots(&state::lock(&state.settings));
    timetrack::start(app);
    moments::start(app);
    inbox::start(app);
    recipes::start(app);
    brief::start(app, state.data_dir.join("last-brief"), roots);
    search::reindex_folders(app);
    search::start_embedder(app);
    updates::start(app);
    files::start_weekly_check(app);
    layout::start(app);
    mcp::start(app, state.mcp_token.clone());
    meetings::start(app);
    health::start(app);
    log::info!("features started");
}
