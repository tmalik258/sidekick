//! Ask mode (FR-UI-07): the island itself grows into a panel for commands
//! and chat. A global shortcut (Alt+Space by default) or the island's Ask
//! button opens it; Esc or clicking anywhere else closes it.

use std::sync::Mutex;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, WindowEvent};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::ai;
use crate::island::{self, LABEL};
use crate::state::{AppState, lock};
use crate::suggestions;

pub const OPEN_EVENT: &str = "ask://open";
pub const CLOSE_EVENT: &str = "ask://close";

/// Focus can bounce right after opening (the shortcut's own key release, the
/// shell settling); a blur this soon after opening does not close it.
const BLUR_GRACE: Duration = Duration::from_millis(500);
/// A blur only closes Ask mode if focus is still gone a moment later. The
/// shortcut's own key grab blurs the window briefly, and the press that
/// follows must toggle Ask mode closed rather than race the blur.
const BLUR_SETTLE: Duration = Duration::from_millis(150);
static OPENED_AT: Mutex<Option<Instant>> = Mutex::new(None);

fn just_opened() -> bool {
    OPENED_AT
        .lock()
        .ok()
        .and_then(|t| *t)
        .is_some_and(|t| t.elapsed() < BLUR_GRACE)
}

/// Sent to the island each time ask mode opens.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Open {
    pub context: ai::Context,
    /// Text to put in the input, e.g. "Explain this error".
    pub prompt: Option<String>,
    /// Send `prompt` right away, with the clipboard attached.
    pub ask: bool,
}

pub fn is_open(app: &AppHandle) -> bool {
    app.state::<AppState>().ask_open.load(Ordering::SeqCst)
}

pub fn setup(app: &AppHandle, hotkey: &str) {
    if let Some(window) = app.get_webview_window(LABEL) {
        let app = app.clone();
        let w = window.clone();
        window.on_window_event(move |event| {
            if let WindowEvent::Focused(false) = event
                && is_open(&app)
            {
                if just_opened() {
                    // Take focus back instead of closing.
                    let _ = w.set_focus();
                } else {
                    let app = app.clone();
                    let w = w.clone();
                    tauri::async_runtime::spawn(async move {
                        tokio::time::sleep(BLUR_SETTLE).await;
                        if !w.is_focused().unwrap_or(false) {
                            close(&app);
                        }
                    });
                }
            }
        });
    }
    if let Err(err) = register(app, hotkey) {
        log::warn!("could not register the Ask shortcut {hotkey}: {err}");
    }
}

/// Swaps the Ask shortcut. Returns an error the settings UI can show.
pub fn register(app: &AppHandle, hotkey: &str) -> Result<(), String> {
    let shortcut: Shortcut = hotkey
        .parse()
        .map_err(|e| format!("{hotkey} is not a valid shortcut: {e}"))?;
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    gs.on_shortcut(shortcut, |app, _shortcut, event| {
        if event.state == ShortcutState::Pressed {
            toggle(app);
        }
    })
    .map_err(|e| format!("{hotkey} could not be registered (another app may use it): {e}"))?;
    log::info!("Ask shortcut: {hotkey}");
    Ok(())
}

pub fn toggle(app: &AppHandle) {
    if is_open(app) {
        close(app);
    } else {
        open(app, Open::default());
    }
}

pub fn open(app: &AppHandle, mut open: Open) {
    let Some(window) = app.get_webview_window(LABEL) else {
        return;
    };
    // Captured before the island takes focus, so it describes the app the
    // user was in.
    open.context = ai::context(app);
    if let Ok(mut t) = OPENED_AT.lock() {
        *t = Some(Instant::now());
    }
    let state = app.state::<AppState>();
    state.ask_open.store(true, Ordering::SeqCst);
    // Typing goes to the island, so it must take clicks and focus now.
    let _ = window.set_ignore_cursor_events(false);
    if *lock(&state.island_hidden) {
        let _ = window.emit(island::VISIBLE_EVENT, true);
    }
    let _ = window.emit(OPEN_EVENT, open);
    if let Err(err) = window.set_focus() {
        log::warn!("could not focus the island: {err}");
    }
}

pub fn close(app: &AppHandle) {
    let state = app.state::<AppState>();
    if !state.ask_open.swap(false, Ordering::SeqCst) {
        return;
    }
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.emit(CLOSE_EVENT, ());
        if *lock(&state.island_hidden) {
            let _ = window.emit(island::VISIBLE_EVENT, false);
        }
    }
    // Suggestions that waited while the user was typing can show now.
    suggestions::resume(app);
}
