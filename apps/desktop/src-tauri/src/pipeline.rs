//! Consumes the event bus: stores every event, runs the skill engine, and
//! hands proposals to the island.

use std::time::{Duration, Instant};

use chrono::Utc;
use sidekick_core::{Event, MascotEvent, Pause};
use sidekick_sensors::{DownloadsSensor, IdleSensor, PAIR_REQUEST, PAIRED, WindowSensor};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::broadcast::error::RecvError;

use crate::commands;
use crate::decide;
use crate::island;
use crate::mascot;
use crate::search;
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

/// Hook events from the Claude Code and Codex runs Ask starts itself: the
/// answer is already in Ask, so they make no "finished" card.
fn from_ask(app: &AppHandle, event: &Event) -> bool {
    if !(event.kind.starts_with("claude.") || event.kind.starts_with("codex.")) {
        return false;
    }
    let Some(cwd) = event.payload["cwd"].as_str().filter(|c| !c.is_empty()) else {
        return false;
    };
    let dir = &app.state::<AppState>().ai_workdir;
    let same = |p: &std::path::Path| {
        let norm = |s: &str| s.replace('\\', "/").trim_end_matches('/').to_lowercase();
        norm(cwd) == norm(&p.to_string_lossy())
    };
    same(dir) || same(&dir.join("codex"))
}

async fn handle(app: &AppHandle, mut event: Event) {
    if from_ask(app, &event) {
        return;
    }
    if crate::privacy::check(app, &event) {
        // Still remember which app is in front, so its copies are ignored
        // too, and keep the island out of fullscreen apps.
        if event.kind == WindowSensor::EVENT_KIND {
            island::follow_fullscreen(app, &event.payload);
            *lock(&app.state::<AppState>().last_window) = Some(event.payload.clone());
            timetrack::on_away(app);
        }
        return;
    }
    // Pairing finished: tell the UI at once (welcome waiting guide, Connections).
    if event.kind == PAIRED {
        let _ = app.emit("browsers://changed", ());
        return;
    }
    // Before onboarding is done Sidekick only onboards: the island follows
    // the window, and the extension can pair. Nothing is stored or suggested.
    if !lock(&app.state::<AppState>().settings).onboarded {
        if event.kind == WindowSensor::EVENT_KIND {
            island::follow_active_monitor(app, &event.payload);
            island::follow_fullscreen(app, &event.payload);
        } else if event.kind == PAIR_REQUEST {
            propose(app, &event);
        }
        return;
    }
    store(app, event.clone()).await;
    search::index_event(app, &event);
    crate::stuck::observe(app, &event);
    crate::clone::observe(app, &event);
    crate::routines::observe(app, &event);

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
            crate::moments::on_back(app, event.payload["away_secs"].as_u64().unwrap_or(0));
            if let Some(w) = lock(&app.state::<AppState>().last_window).clone() {
                timetrack::on_window(app, &w);
            }
        }
    }

    if event.kind == WindowSensor::EVENT_KIND {
        island::follow_active_monitor(app, &event.payload);
        island::follow_fullscreen(app, &event.payload);
        *lock(&app.state::<AppState>().last_window) = Some(event.payload.clone());
        timetrack::on_window(app, &event.payload);
        crate::projects::on_window(app, &event.payload);
        crate::git_watch::on_window(app, &event.payload);
        crate::moments::on_window(&event.payload);
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

    if event.kind == sidekick_sensors::calendar::CalendarSensor::SOON {
        crate::moments::enrich_meeting(app, &mut event);
    }

    propose(app, &event);
}

/// Runs the skills on `event` and offers what matched.
fn propose(app: &AppHandle, event: &Event) {
    if let Some(proposal) = evaluate(app, event)
        && !crate::learn::is_muted(app, &proposal.skill_id)
    {
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
            crate::chat_prune::run(&app).await;
        }
    });
}
