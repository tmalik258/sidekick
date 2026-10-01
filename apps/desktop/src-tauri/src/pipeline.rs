//! Consumes the event bus: stores every event, runs the skill engine, and
//! hands proposals to the island.

use std::time::{Duration, Instant};

use chrono::Utc;
use sidekick_core::{Event, MascotEvent, Pause};
use sidekick_sensors::{DownloadsSensor, IdleSensor, WindowSensor};
use tauri::{AppHandle, Manager};
use tokio::sync::broadcast::error::RecvError;

use crate::commands;
use crate::decide;
use crate::island;
use crate::mascot;
use crate::state::{AppEnv, AppState, executor, lock};
use crate::suggestions;
use crate::timetrack;

/// Event kind published by the debug panel's "Emit test event" button.
pub const DEBUG_MANUAL_KIND: &str = "debug.manual";

const NOTICE_HOLD: Duration = Duration::from_millis(1200);
const PAUSE_CHECK_EVERY: Duration = Duration::from_secs(5);
const PRUNE_EVERY: Duration = Duration::from_secs(60 * 60);
const MAX_STORED_EVENTS: u32 = 100_000;

pub fn start(app: &AppHandle) {
    spawn_consumer(app.clone());
    spawn_pause_expiry(app.clone());
    spawn_pruner(app.clone());
}

fn spawn_consumer(app: AppHandle) {
    let mut rx = app.state::<AppState>().bus.subscribe();
    tauri::async_runtime::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(event) => handle(&app, event).await,
                Err(RecvError::Lagged(skipped)) => {
                    log::warn!("event consumer lagged, skipped {skipped} events")
                }
                Err(RecvError::Closed) => break,
            }
        }
    });
}

async fn handle(app: &AppHandle, event: Event) {
    store(app, event.clone()).await;

    if event.kind == IdleSensor::IDLE || event.kind == IdleSensor::ACTIVE {
        let away = event.kind == IdleSensor::IDLE;
        app.state::<AppState>()
            .away
            .store(away, std::sync::atomic::Ordering::Relaxed);
        if away {
            timetrack::on_away(app);
        } else {
            // Anything that came in while the user was away shows now.
            suggestions::welcome_back(app);
            if let Some(w) = lock(&app.state::<AppState>().last_window).clone() {
                timetrack::on_window(app, &w);
            }
        }
    }

    if event.kind == WindowSensor::EVENT_KIND {
        island::follow_fullscreen(app, &event.payload);
        *lock(&app.state::<AppState>().last_window) = Some(event.payload.clone());
        timetrack::on_window(app, &event.payload);
    }

    if event.kind == DEBUG_MANUAL_KIND && mascot::dispatch(app, MascotEvent::SkillMatched).is_some()
    {
        mascot::after(app, NOTICE_HOLD, MascotEvent::ConditionsFailed);
        return;
    }

    // A conversion saved into Downloads is not a new download.
    if event.kind == DownloadsSensor::EVENT_KIND
        && event.payload["path"]
            .as_str()
            .is_some_and(|p| suggestions::is_own_file(app, p))
    {
        return;
    }

    if let Some(proposal) = evaluate(app, &event) {
        // Ranking may ask a model, so it runs beside the event loop.
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let proposal = decide::rank(&app, proposal).await;
            suggestions::offer(&app, proposal);
        });
    }
}

fn evaluate(app: &AppHandle, event: &Event) -> Option<sidekick_skills::Proposal> {
    let state = app.state::<AppState>();
    let settings = lock(&state.settings).clone();
    let exec = executor(&state);
    let env = AppEnv {
        settings: &settings,
        ai_ready: state.ai_ready.load(std::sync::atomic::Ordering::Relaxed),
        caps: exec.capabilities(),
        storage: &state.storage,
    };
    lock(&state.engine).evaluate(event, &env, Instant::now())
}

async fn store(app: &AppHandle, event: Event) {
    let storage = app.state::<AppState>().storage.clone();
    let result =
        tauri::async_runtime::spawn_blocking(move || lock(&storage).insert_event(&event)).await;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(err)) => log::error!("could not store event: {err}"),
        Err(err) => log::error!("storage task failed: {err}"),
    }
}

/// Lifts a timed pause once it expires.
fn spawn_pause_expiry(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(PAUSE_CHECK_EVERY).await;
            let state = app.state::<AppState>();
            let expired = {
                let settings = lock(&state.settings);
                matches!(settings.pause, Pause::Until(_)) && !settings.pause.is_active(Utc::now())
            };
            if expired && let Err(err) = commands::set_pause(&app, Pause::None) {
                log::error!("could not lift expired pause: {err}");
            }
        }
    });
}

fn spawn_pruner(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(PRUNE_EVERY).await;
            let storage = app.state::<AppState>().storage.clone();
            let pruned = tauri::async_runtime::spawn_blocking(move || {
                lock(&storage).prune_events(MAX_STORED_EVENTS)
            })
            .await;
            if let Ok(Ok(n)) = pruned
                && n > 0
            {
                log::info!("pruned {n} old events");
            }
        }
    });
}
