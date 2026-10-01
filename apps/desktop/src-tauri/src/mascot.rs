//! Drives the mascot state machine and tells the UI about every transition.

use std::sync::atomic::Ordering;
use std::time::Duration;

use sidekick_core::{MascotEvent, MascotState, Transition};
use tauri::{AppHandle, Emitter, Manager};

use crate::state::{AppState, lock};

pub const STATE_EVENT: &str = "mascot://state";

const SUCCESS_HOLD: Duration = Duration::from_millis(1500);
const ERROR_HOLD: Duration = Duration::from_secs(4);

pub fn current(app: &AppHandle) -> MascotState {
    lock(&app.state::<AppState>().mascot).state()
}

/// Applies an event; invalid events for the current state are ignored.
pub fn dispatch(app: &AppHandle, event: MascotEvent) -> Option<Transition> {
    let transition = lock(&app.state::<AppState>().mascot).dispatch(event);
    match transition {
        Some(t) => entered(app, t),
        None => log::debug!("mascot ignored {event:?} in {:?}", current(app)),
    }
    transition
}

/// Jumps to a state regardless of the machine's rules (debug panel only).
pub fn force(app: &AppHandle, state: MascotState) -> Transition {
    let t = lock(&app.state::<AppState>().mascot).force(state);
    entered(app, t);
    t
}

/// Dispatches `event` after `delay`, unless the mascot changed state in the
/// meantime.
pub fn after(app: &AppHandle, delay: Duration, event: MascotEvent) {
    let epoch = app.state::<AppState>().mascot_epoch.load(Ordering::SeqCst);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(delay).await;
        if app.state::<AppState>().mascot_epoch.load(Ordering::SeqCst) == epoch {
            dispatch(&app, event);
        }
    });
}

fn entered(app: &AppHandle, t: Transition) {
    app.state::<AppState>()
        .mascot_epoch
        .fetch_add(1, Ordering::SeqCst);
    if let Err(err) = app.emit(STATE_EVENT, t) {
        log::warn!("could not emit mascot state: {err}");
    }
    match t.state {
        MascotState::Success => after(app, SUCCESS_HOLD, MascotEvent::SuccessElapsed),
        MascotState::Error => after(app, ERROR_HOLD, MascotEvent::ErrorDismissed),
        _ => {}
    }
}
