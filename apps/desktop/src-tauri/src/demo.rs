//! A scripted suggestion flow for the debug panel. It exercises every island
//! state (noticing, suggesting, working, success or error) before real skills
//! exist. Removed or hidden once P1 skills ship.

use std::time::Duration;

use sidekick_core::{MascotEvent, MascotState};
use tauri::{AppHandle, Emitter, Manager};

use crate::mascot;
use crate::state::{AppState, Suggestion, lock};

pub const SUGGESTION_NEW: &str = "suggestion://new";
pub const SUGGESTION_CLEAR: &str = "suggestion://clear";

const NOTICE_DELAY: Duration = Duration::from_millis(900);
const WORK_DELAY: Duration = Duration::from_millis(1400);
const FAIL_OPTION: usize = 2;

pub fn start(app: &AppHandle) -> Result<(), String> {
    if mascot::dispatch(app, MascotEvent::SkillMatched).is_none() {
        return Err(format!("mascot is busy ({:?})", mascot::current(app)));
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(NOTICE_DELAY).await;
        if mascot::current(&app) != MascotState::Noticing {
            return;
        }
        let suggestion = Suggestion {
            id: ulid::Ulid::new().to_string(),
            title: "Dev server on localhost:3000".into(),
            detail: "Demo suggestion. Real skills arrive in P1.".into(),
            options: vec![
                "Open in Chrome".into(),
                "Open in Zen".into(),
                "Simulate failure".into(),
            ],
        };
        *lock(&app.state::<AppState>().suggestion) = Some(suggestion.clone());
        if let Err(err) = app.emit(SUGGESTION_NEW, suggestion) {
            log::warn!("could not emit suggestion: {err}");
        }
        mascot::dispatch(&app, MascotEvent::SuggestionReady);
    });
    Ok(())
}

pub fn choose(app: &AppHandle, id: &str, index: usize) -> Result<(), String> {
    let suggestion = take(app, id)?;
    let label = suggestion
        .options
        .get(index)
        .ok_or("option out of range")?
        .clone();
    log::info!("demo option chosen: {label}");
    mascot::dispatch(app, MascotEvent::Picked);
    let outcome = if index == FAIL_OPTION {
        MascotEvent::ActionFailed
    } else {
        MascotEvent::ActionDone
    };
    mascot::after(app, WORK_DELAY, outcome);
    Ok(())
}

pub fn dismiss(app: &AppHandle, id: &str, reason: &str) -> Result<(), String> {
    take(app, id)?;
    log::info!("demo suggestion dismissed: {reason}");
    mascot::dispatch(app, MascotEvent::Dismissed);
    Ok(())
}

/// Removes the current suggestion if `id` matches, and tells the UI.
fn take(app: &AppHandle, id: &str) -> Result<Suggestion, String> {
    let state = app.state::<AppState>();
    let taken = {
        let mut current = lock(&state.suggestion);
        match current.as_ref() {
            Some(s) if s.id == id => current.take(),
            _ => None,
        }
    };
    let suggestion = taken.ok_or("suggestion is no longer active")?;
    let _ = app.emit(SUGGESTION_CLEAR, &suggestion.id);
    Ok(suggestion)
}
