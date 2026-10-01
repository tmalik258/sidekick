mod ai;
mod ask;
mod brief;
mod browser;
mod commands;
mod decide;
mod fathom;
mod files;
mod island;
mod layout;
mod learn;
mod mascot;
mod mcp;
mod pipeline;
mod projects;
mod screen;
mod search;
mod state;
mod suggestions;
mod timetrack;
mod tray;
mod undo;
mod updates;
mod voice;
mod windows;

use std::error::Error;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use chrono::Utc;
use sidekick_actions::{Capabilities, Executor};
use sidekick_core::{EventBus, MascotEvent, Settings, Storage};
use sidekick_sensors::{
    BrowserBridge, BrowserSensor, ClaudeCodeSensor, ClipboardSensor, DownloadsSensor,
    HeartbeatSensor, IdleSensor, PortsSensor, ReposSensor, Sensor, SensorGate, SystemSensor,
    WindowSensor,
};
use sidekick_skills::Engine;
use tauri::{AppHandle, Manager};

use crate::state::AppState;

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);

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
            commands::suggestion_current,
            commands::suggestion_choose,
            commands::suggestion_dismiss,
            commands::events_recent,
            commands::open_settings,
            commands::calendar_today,
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
            commands::debug_set_state,
            commands::debug_emit_event,
            commands::debug_demo_flow,
            commands::skills_list,
            commands::skill_set,
            commands::capabilities_get,
            commands::choices_reset,
            commands::actions_recent,
            commands::reveal_path,
            commands::ai_status,
            commands::ai_chat,
            commands::ai_cancel,
            commands::ask_open,
            commands::ask_close,
            commands::browser_info,
            commands::time_today,
            commands::skill_install,
            commands::search,
            commands::search_status,
            commands::search_reindex,
            commands::open_reference,
            commands::mcp_info,
            commands::action_undo,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Sidekick");
}

fn setup(app: &AppHandle) -> Result<(), Box<dyn Error>> {
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
    let db_path = data_dir.join("sidekick.db");
    let scratch_dir = app.path().app_cache_dir()?;

    let settings = Settings::load(&settings_path);
    let storage = Storage::open(&db_path)?;
    let bus = EventBus::default();
    let (gate_handle, gate) = SensorGate::new(state::gate_state(&settings, Utc::now()));
    let paused = settings.pause.is_active(Utc::now());
    let hotkey = settings.palette_hotkey.clone();
    let repos = ReposSensor {
        roots: if settings.code_folders.is_empty() {
            ReposSensor::default_roots()
        } else {
            settings.code_folders.iter().map(Into::into).collect()
        },
        hour: settings.end_of_day_hour,
    };
    let browser_token = browser::load_or_create_token(&data_dir);
    let mcp_token = browser::load_or_create_secret(&data_dir, "mcp-token");
    let bridge = BrowserBridge::default();
    let calendar = sidekick_sensors::Calendar::default();
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
        island_hidden: Mutex::default(),
        hovered: Default::default(),
        last_window: Mutex::default(),
        chats: Mutex::default(),
        ai_workdir: data_dir.join("claude-workspace"),
        voice: voice::Voice::new(data_dir.join("voice-models")),
        calendar: calendar.clone(),
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
        mcp_token: mcp_token.clone(),
    });

    pipeline::start(app);
    timetrack::start(app);
    voice::refresh(app);
    if !settings_onboarded(app) {
        // Give the island a moment to load before it grows into the welcome.
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_millis(1500)).await;
            ask::open(
                &app,
                ask::Open {
                    view: Some("welcome"),
                    ..Default::default()
                },
            );
        });
    }
    brief::start(app, data_dir.join("last-brief"), repos.roots.clone());
    search::reindex_folders(app);
    search::start_embedder(app);
    updates::start(app);
    files::start_weekly_check(app);
    layout::start(app);
    mcp::start(app, mcp_token);
    tauri::async_runtime::spawn(async move {
        let sensors: Vec<Box<dyn Sensor>> = vec![
            Box::new(DownloadsSensor::new()),
            Box::new(DownloadsSensor::screenshots()),
            Box::new(PortsSensor),
            Box::new(ClipboardSensor),
            Box::new(WindowSensor),
            Box::new(ClaudeCodeSensor {
                port: ClaudeCodeSensor::DEFAULT_PORT,
            }),
            Box::new(BrowserSensor {
                port: BrowserSensor::DEFAULT_PORT,
                token: browser_token,
                bridge,
            }),
            Box::new(SystemSensor),
            Box::new(sidekick_sensors::CalendarSensor { state: calendar }),
            Box::new(repos),
            Box::new(IdleSensor::default()),
            Box::new(HeartbeatSensor::new(HEARTBEAT_INTERVAL)),
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
