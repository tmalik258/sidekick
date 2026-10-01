//! Consumes the event bus: stores every event and, in P0, turns debug events
//! into mascot reactions. Skills replace the reaction part in P1.

use std::time::Duration;

use chrono::Utc;
use sidekick_core::{Event, MascotEvent, Pause};
use sidekick_sensors::HeartbeatSensor;
use tauri::{AppHandle, Manager};
use tokio::sync::broadcast::error::RecvError;

use crate::commands;
use crate::mascot;
use crate::state::{AppState, lock};

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

    if (event.kind == HeartbeatSensor::EVENT_KIND || event.kind == DEBUG_MANUAL_KIND)
        && mascot::dispatch(app, MascotEvent::SkillMatched).is_some()
    {
        // No skills yet, so conditions always fail and the mascot settles.
        mascot::after(app, NOTICE_HOLD, MascotEvent::ConditionsFailed);
    }
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
