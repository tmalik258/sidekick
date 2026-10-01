mod commands;
mod island;
mod mascot;
mod pipeline;
mod state;
mod suggestions;
mod tray;
mod windows;

use std::error::Error;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use chrono::Utc;
use sidekick_actions::{Capabilities, Executor};
use sidekick_core::{EventBus, MascotEvent, Settings, Storage};
use sidekick_sensors::{
    ClipboardSensor, DownloadsSensor, HeartbeatSensor, PortsSensor, Sensor, SensorGate,
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
            commands::debug_set_state,
            commands::debug_emit_event,
            commands::debug_demo_flow,
            commands::skills_list,
            commands::skill_set,
            commands::capabilities_get,
            commands::choices_reset,
            commands::actions_recent,
            commands::reveal_path,
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
    let db_path = app.path().app_data_dir()?.join("sidekick.db");

    let settings = Settings::load(&settings_path);
    let storage = Storage::open(&db_path)?;
    let bus = EventBus::default();
    let (gate_handle, gate) = SensorGate::new(state::gate_state(&settings, Utc::now()));
    let paused = settings.pause.is_active(Utc::now());

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
    });

    pipeline::start(app);
    tauri::async_runtime::spawn(async move {
        let sensors: Vec<Box<dyn Sensor>> = vec![
            Box::new(DownloadsSensor::new()),
            Box::new(PortsSensor),
            Box::new(ClipboardSensor),
            Box::new(WindowSensor),
            Box::new(HeartbeatSensor::new(HEARTBEAT_INTERVAL)),
        ];
        sidekick_sensors::spawn_all(sensors, &bus, &gate);
    });

    island::setup(app)?;
    tray::create(app)?;

    if paused {
        mascot::dispatch(app, MascotEvent::Rest);
    }
    log::info!("Sidekick started");
    Ok(())
}
