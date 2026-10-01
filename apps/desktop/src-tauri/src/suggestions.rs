//! Shows proposals from the skill engine on the island, one at a time, and
//! runs the option the user picks (or the first safe one, for Auto skills).

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use chrono::Utc;
use serde::Serialize;
use sidekick_actions::is_safe;
use sidekick_core::{ActionRecord, MascotEvent, MascotState};
use sidekick_skills::{Proposal, ProposedOption, Trust};
use tauri::{AppHandle, Emitter, Manager};

use crate::mascot;
use crate::state::{Active, AppState, Queued, Suggestion, executor, lock};

pub const SUGGESTION_NEW: &str = "suggestion://new";
pub const SUGGESTION_CLEAR: &str = "suggestion://clear";
pub const ACTION_RESULT: &str = "action://result";

/// Queued proposals older than this are dropped: the moment has passed.
const STALE_AFTER: Duration = Duration::from_secs(90);
const MAX_QUEUE: usize = 5;
/// Pause between one suggestion ending and the next appearing.
const NEXT_AFTER_DISMISS: Duration = Duration::from_millis(450);
const NEXT_AFTER_ACTION: Duration = Duration::from_millis(1900);
const EXPIRY_TICK: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionResult {
    pub ok: bool,
    pub message: String,
    pub path: Option<String>,
    pub auto: bool,
}

/// Shows a proposal now, or queues it while the island is busy.
pub fn offer(app: &AppHandle, proposal: Proposal) {
    let state = app.state::<AppState>();
    let busy = lock(&state.active).is_some() || mascot::current(app) != MascotState::Idle;
    if busy {
        let mut queue = lock(&state.queue);
        // A newer copy of the same suggestion replaces the waiting one;
        // different ones (two servers, two downloads) all stay queued.
        queue.retain(|q| {
            !(q.proposal.skill_id == proposal.skill_id && q.proposal.title == proposal.title)
        });
        if queue.len() >= MAX_QUEUE {
            queue.pop_front();
        }
        queue.push_back(Queued {
            proposal,
            at: Instant::now(),
        });
        return;
    }
    show(app, proposal);
}

fn show(app: &AppHandle, proposal: Proposal) {
    if mascot::dispatch(app, MascotEvent::SkillMatched).is_none() {
        // Raced with another state change; try again shortly.
        let app = app.clone();
        lock(&app.state::<AppState>().queue).push_front(Queued {
            proposal,
            at: Instant::now(),
        });
        schedule_next(&app, NEXT_AFTER_DISMISS);
        return;
    }
    let ui = Suggestion {
        id: ulid::Ulid::new().to_string(),
        skill_id: proposal.skill_id.clone(),
        title: proposal.title.clone(),
        detail: proposal.detail.clone(),
        options: proposal.options.iter().map(|o| o.label.clone()).collect(),
    };
    let auto = proposal.trust == Trust::Auto
        && proposal.options.first().is_some_and(|o| is_safe(&o.action));
    let id = ui.id.clone();
    log::info!(
        "suggesting {}: {} [{}]",
        ui.skill_id,
        ui.title,
        ui.options.join(", ")
    );
    *lock(&app.state::<AppState>().active) = Some(Active {
        ui: ui.clone(),
        proposal,
    });
    let _ = app.emit(SUGGESTION_NEW, ui);
    mascot::dispatch(app, MascotEvent::SuggestionReady);
    if auto {
        let _ = run_choice(app, &id, 0, true);
    } else {
        expire_when_ignored(app, id);
    }
}

/// Dismisses a suggestion nobody looked at (FR-UI-02). Hovering the island
/// restarts the countdown. Lives here rather than in the UI so a stalled
/// webview can never block the queue.
fn expire_when_ignored(app: &AppHandle, id: String) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut idle = Duration::ZERO;
        loop {
            tokio::time::sleep(EXPIRY_TICK).await;
            let state = app.state::<AppState>();
            let still_active = lock(&state.active).as_ref().is_some_and(|a| a.ui.id == id);
            if !still_active {
                return;
            }
            let limit = Duration::from_secs(u64::from(lock(&state.settings).collapse_after_secs));
            idle = if state.hovered.load(Ordering::Relaxed) {
                Duration::ZERO
            } else {
                idle + EXPIRY_TICK
            };
            if idle >= limit {
                let _ = dismiss(&app, &id, "timeout");
                return;
            }
        }
    });
}

pub fn choose(app: &AppHandle, id: &str, index: usize) -> Result<(), String> {
    run_choice(app, id, index, false)
}

fn run_choice(app: &AppHandle, id: &str, index: usize, auto: bool) -> Result<(), String> {
    let active = take(app, id)?;
    let option = active
        .proposal
        .options
        .get(index)
        .cloned()
        .ok_or("option out of range")?;
    if !auto && let Some(key) = &active.proposal.remember {
        let ts = Utc::now().to_rfc3339();
        if let Err(err) =
            lock(&app.state::<AppState>().storage).record_choice(key, &option.label, &ts)
        {
            log::warn!("could not remember choice: {err}");
        }
    }
    mascot::dispatch(app, MascotEvent::Picked);

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = execute(&app, &option).await;
        log_action(&app, &active.proposal, &option, &result, auto);
        let payload = match &result {
            Ok(outcome) => ActionResult {
                ok: true,
                message: outcome.message.clone(),
                path: outcome.path.clone(),
                auto,
            },
            Err(message) => ActionResult {
                ok: false,
                message: message.clone(),
                path: None,
                auto,
            },
        };
        let _ = app.emit(ACTION_RESULT, &payload);
        mascot::dispatch(
            &app,
            if payload.ok {
                MascotEvent::ActionDone
            } else {
                MascotEvent::ActionFailed
            },
        );
        schedule_next(&app, NEXT_AFTER_ACTION);
    });
    Ok(())
}

async fn execute(
    app: &AppHandle,
    option: &ProposedOption,
) -> Result<sidekick_actions::Outcome, String> {
    let exec = executor(&app.state::<AppState>());
    exec.run(&option.action, &option.args)
        .await
        .map_err(|e| e.to_string())
}

fn log_action(
    app: &AppHandle,
    proposal: &Proposal,
    option: &ProposedOption,
    result: &Result<sidekick_actions::Outcome, String>,
    auto: bool,
) {
    let record = ActionRecord {
        ts: Utc::now().to_rfc3339(),
        skill_id: option.skill_id.clone(),
        action: option.action.clone(),
        label: option.label.clone(),
        ok: result.is_ok(),
        message: match result {
            Ok(o) => o.message.clone(),
            Err(e) => e.clone(),
        },
        auto,
    };
    log::info!(
        "{} -> {} ({}): {}",
        proposal.skill_id,
        record.label,
        record.action,
        record.message
    );
    if let Err(err) = lock(&app.state::<AppState>().storage).log_action(&record) {
        log::warn!("could not log action: {err}");
    }
}

pub fn dismiss(app: &AppHandle, id: &str, reason: &str) -> Result<(), String> {
    let active = take(app, id)?;
    log::info!(
        "suggestion from {} dismissed: {reason}",
        active.proposal.skill_id
    );
    mascot::dispatch(app, MascotEvent::Dismissed);
    schedule_next(app, NEXT_AFTER_DISMISS);
    Ok(())
}

/// Removes the active suggestion if `id` matches, and tells the UI.
fn take(app: &AppHandle, id: &str) -> Result<Active, String> {
    let state = app.state::<AppState>();
    let taken = {
        let mut current = lock(&state.active);
        match current.as_ref() {
            Some(a) if a.ui.id == id => current.take(),
            _ => None,
        }
    };
    let active = taken.ok_or("suggestion is no longer active")?;
    let _ = app.emit(SUGGESTION_CLEAR, &active.ui.id);
    Ok(active)
}

pub fn current(app: &AppHandle) -> Option<Suggestion> {
    lock(&app.state::<AppState>().active)
        .as_ref()
        .map(|a| a.ui.clone())
}

/// Shows the next queued proposal once the island is free again.
fn schedule_next(app: &AppHandle, after: Duration) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(after).await;
        let state = app.state::<AppState>();
        if lock(&state.active).is_some() || mascot::current(&app) != MascotState::Idle {
            // Still busy: the action that finishes will schedule again.
            if mascot::current(&app) == MascotState::Sleeping {
                lock(&state.queue).clear();
            }
            return;
        }
        let next = {
            let mut queue = lock(&state.queue);
            queue.retain(|q| q.at.elapsed() < STALE_AFTER);
            queue.pop_front()
        };
        if let Some(q) = next {
            show(&app, q.proposal);
        }
    });
}

/// A scripted suggestion for the debug panel and tray.
pub fn demo(app: &AppHandle) {
    let option = |label: &str, action: &str, message: &str| ProposedOption {
        label: label.into(),
        action: action.into(),
        args: serde_json::json!({ "message": message }),
        skill_id: "debug.demo".into(),
    };
    offer(
        app,
        Proposal {
            skill_id: "debug.demo".into(),
            skill_ids: vec!["debug.demo".into()],
            title: "Demo suggestion".into(),
            detail: "Pick an option to see the flow.".into(),
            options: vec![
                option("Succeed", "noop", "That worked"),
                option("Fail", "fail", "Simulated failure"),
            ],
            trust: Trust::Suggest,
            remember: None,
        },
    );
}
