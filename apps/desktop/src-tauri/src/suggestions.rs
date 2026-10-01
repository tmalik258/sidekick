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
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

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
/// Give the user a moment after they return before showing anything.
const WELCOME_BACK: Duration = Duration::from_millis(1500);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionResult {
    pub ok: bool,
    pub message: String,
    pub path: Option<String>,
    pub auto: bool,
    /// Set when the action can be undone (it created a file).
    pub undo_id: Option<i64>,
}

const DIGITS: [Code; 10] = [
    Code::Digit0,
    Code::Digit1,
    Code::Digit2,
    Code::Digit3,
    Code::Digit4,
    Code::Digit5,
    Code::Digit6,
    Code::Digit7,
    Code::Digit8,
    Code::Digit9,
];

/// Alt+1..9 pick an option and Alt+0 is "Not now". They are global, because
/// the island never takes focus, and exist only while a suggestion is showing
/// so they never steal keys otherwise.
fn bind_keys(app: &AppHandle, options: usize) {
    let gs = app.global_shortcut();
    for (n, code) in DIGITS.iter().enumerate().take(options.min(9) + 1) {
        let shortcut = Shortcut::new(Some(Modifiers::ALT), *code);
        let result = gs.on_shortcut(shortcut, move |app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                // Off the handler's thread: picking unregisters these very
                // shortcuts, which must not happen from inside their callback.
                let app = app.clone();
                tauri::async_runtime::spawn(async move { pick(&app, n) });
            }
        });
        if let Err(err) = result {
            log::warn!("could not register Alt+{n} (another app may use it): {err}");
        }
    }
}

fn unbind_keys(app: &AppHandle) {
    let gs = app.global_shortcut();
    for digit in DIGITS {
        let _ = gs.unregister(Shortcut::new(Some(Modifiers::ALT), digit));
    }
}

/// Registers the keys again for the active suggestion, after something else
/// cleared every shortcut (changing the Ask shortcut does).
pub fn rebind_keys(app: &AppHandle) {
    if let Some(s) = current(app) {
        bind_keys(app, s.options.len());
    }
}

/// Alt+N: option N, or "Not now" for 0.
fn pick(app: &AppHandle, n: usize) {
    log::info!("shortcut Alt+{n}");
    let Some(s) = current(app) else { return };
    let result = if n == 0 {
        dismiss(app, &s.id, "shortcut")
    } else if n <= s.options.len() {
        choose(app, &s.id, n - 1)
    } else {
        return;
    };
    if let Err(err) = result {
        log::debug!("shortcut ignored: {err}");
    }
}

/// Shows a proposal now, or queues it while the island is busy.
pub fn offer(app: &AppHandle, proposal: Proposal) {
    let state = app.state::<AppState>();
    // While the user is away, suggestions wait instead of showing to no one.
    let busy = lock(&state.active).is_some()
        || mascot::current(app) != MascotState::Idle
        || state.away.load(std::sync::atomic::Ordering::Relaxed)
        || state.ask_open.load(std::sync::atomic::Ordering::SeqCst);
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
    let _ = app.emit(SUGGESTION_NEW, &ui);
    mascot::dispatch(app, MascotEvent::SuggestionReady);
    if auto {
        let _ = run_choice(app, &id, 0, true);
    } else {
        bind_keys(app, ui.options.len());
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
    let follow_up = crate::learn::on_accept(app, &active.proposal, index, auto);

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = execute(&app, &option).await;
        if let Ok(sidekick_actions::Outcome {
            path: Some(path), ..
        }) = &result
        {
            remember_own_file(&app, path);
        }
        let id = log_action(&app, &active.proposal, &option, &result, auto);
        let payload = match &result {
            Ok(outcome) => ActionResult {
                ok: true,
                message: outcome.message.clone(),
                path: outcome.path.clone(),
                auto,
                undo_id: id.filter(|_| {
                    crate::undo::undo_path(&option.action, outcome.path.as_deref()).is_some()
                }),
            },
            Err(message) => ActionResult {
                ok: false,
                message: message.clone(),
                path: None,
                auto,
                undo_id: None,
            },
        };
        let _ = app.emit(ACTION_RESULT, &payload);
        app.state::<AppState>().linger.store(
            payload.ok && (payload.path.is_some() || payload.undo_id.is_some()),
            std::sync::atomic::Ordering::SeqCst,
        );
        mascot::dispatch(
            &app,
            if payload.ok {
                MascotEvent::ActionDone
            } else {
                MascotEvent::ActionFailed
            },
        );
        if let Some(offer) = follow_up {
            offer_later(&app, offer);
        }
        schedule_next(&app, NEXT_AFTER_ACTION);
    });
    Ok(())
}

async fn execute(
    app: &AppHandle,
    option: &ProposedOption,
) -> Result<sidekick_actions::Outcome, String> {
    // App-level actions that need the window system rather than the OS.
    let arg = |name: &str| option.args[name].as_str().filter(|s| !s.is_empty());
    match option.action.as_str() {
        "ask_ai" => {
            crate::ask::open(
                app,
                crate::ask::Open {
                    prompt: Some(arg("prompt").unwrap_or("Help me with this.").to_owned()),
                    ask: true,
                    clipboard: arg("clipboard") != Some("false"),
                    page: arg("page").map(str::to_owned),
                    ..Default::default()
                },
            );
            return Ok(sidekick_actions::Outcome {
                message: "Asking Sidekick".into(),
                path: None,
            });
        }
        "fathom_followup" => {
            return crate::fathom::follow_up(
                app,
                arg("start").unwrap_or_default(),
                arg("title").unwrap_or("the meeting"),
            )
            .await
            .map(|message| sidekick_actions::Outcome {
                message,
                path: None,
            });
        }
        "skill_auto" => {
            let skill = arg("skill").ok_or("no skill")?;
            return crate::learn::make_auto(app, skill).map(|message| sidekick_actions::Outcome {
                message,
                path: None,
            });
        }
        "browser_fill" | "browser_close_duplicates" | "browser_save_session" => {
            return crate::browser::run(app, &option.action, &option.args).await;
        }
        _ => {}
    }
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
) -> Option<i64> {
    let record = ActionRecord {
        id: 0,
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
        undo_path: result
            .as_ref()
            .ok()
            .and_then(|o| crate::undo::undo_path(&option.action, o.path.as_deref())),
        undone: false,
    };
    log::info!(
        "{} -> {} ({}): {}",
        proposal.skill_id,
        record.label,
        record.action,
        record.message
    );
    crate::search::index_action(
        app,
        &record.label,
        &record.message,
        &record.skill_id,
        &record.ts,
    );
    match lock(&app.state::<AppState>().storage).log_action(&record) {
        Ok(id) => Some(id),
        Err(err) => {
            log::warn!("could not log action: {err}");
            None
        }
    }
}

pub fn dismiss(app: &AppHandle, id: &str, reason: &str) -> Result<(), String> {
    let active = take(app, id)?;
    log::info!(
        "suggestion from {} dismissed: {reason}",
        active.proposal.skill_id
    );
    crate::learn::on_dismiss(app, &active.proposal.skill_id, reason);
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
    unbind_keys(app);
    let _ = app.emit(SUGGESTION_CLEAR, &active.ui.id);
    Ok(active)
}

pub fn current(app: &AppHandle) -> Option<Suggestion> {
    lock(&app.state::<AppState>().active)
        .as_ref()
        .map(|a| a.ui.clone())
}

/// Shows the next queued proposal once the island is free again.
/// How long a file Sidekick made is ignored by the downloads sensor.
const OWN_FILE_FOR: Duration = Duration::from_secs(120);

fn remember_own_file(app: &AppHandle, path: &str) {
    let state = app.state::<AppState>();
    let mut own = lock(&state.own_files);
    own.retain(|_, at| at.elapsed() < OWN_FILE_FOR);
    own.insert(std::path::PathBuf::from(path), Instant::now());
}

/// True for a file one of Sidekick's own actions just created.
pub fn is_own_file(app: &AppHandle, path: &str) -> bool {
    lock(&app.state::<AppState>().own_files)
        .get(std::path::Path::new(path))
        .is_some_and(|at| at.elapsed() < OWN_FILE_FOR)
}

/// Queues a follow-up so it shows after the current result.
fn offer_later(app: &AppHandle, proposal: Proposal) {
    lock(&app.state::<AppState>().queue).push_back(Queued {
        proposal,
        at: Instant::now(),
    });
}

/// Shows the next queued suggestion, if any (after a held result).
pub fn resume(app: &AppHandle) {
    schedule_next(app, NEXT_AFTER_DISMISS);
}

/// The user is back: what arrived while they were away counts as fresh.
pub fn welcome_back(app: &AppHandle) {
    let now = Instant::now();
    for q in lock(&app.state::<AppState>().queue).iter_mut() {
        q.at = now;
    }
    schedule_next(app, WELCOME_BACK);
}

fn schedule_next(app: &AppHandle, after: Duration) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(after).await;
        let state = app.state::<AppState>();
        if state.away.load(std::sync::atomic::Ordering::Relaxed)
            || state.ask_open.load(std::sync::atomic::Ordering::SeqCst)
        {
            return;
        }
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
