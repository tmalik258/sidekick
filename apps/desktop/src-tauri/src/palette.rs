//! The command palette window (FR-UI-07): a global shortcut toggles it, and
//! it hides again when it loses focus.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, WindowEvent};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::ai;

pub const LABEL: &str = "palette";
pub const OPEN_EVENT: &str = "palette://open";

/// Focus can bounce right after opening (the shortcut's own key release, the
/// shell settling); a blur this soon after opening does not close it.
const BLUR_GRACE: Duration = Duration::from_millis(500);
static OPENED_AT: Mutex<Option<Instant>> = Mutex::new(None);

fn just_opened() -> bool {
    OPENED_AT
        .lock()
        .ok()
        .and_then(|t| *t)
        .is_some_and(|t| t.elapsed() < BLUR_GRACE)
}

/// Sent to the palette each time it opens.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Open {
    pub context: ai::Context,
    /// Text to put in the input, e.g. "Explain this error".
    pub prompt: Option<String>,
    /// Send `prompt` right away, with the clipboard attached.
    pub ask: bool,
}

pub fn setup(app: &AppHandle, hotkey: &str) {
    if let Some(window) = app.get_webview_window(LABEL) {
        let w = window.clone();
        window.on_window_event(move |event| {
            if let WindowEvent::Focused(false) = event {
                if just_opened() {
                    // Take focus back instead of closing.
                    let _ = w.set_focus();
                } else {
                    let _ = w.hide();
                }
            }
        });
    }
    if let Err(err) = register(app, hotkey) {
        log::warn!("could not register palette shortcut {hotkey}: {err}");
    }
}

/// Swaps the palette shortcut. Returns an error the settings UI can show.
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
    log::info!("palette shortcut: {hotkey}");
    Ok(())
}

pub fn toggle(app: &AppHandle) {
    let Some(window) = app.get_webview_window(LABEL) else {
        return;
    };
    if window.is_visible().unwrap_or(false) && window.is_focused().unwrap_or(false) {
        log::info!("palette hidden by shortcut");
        let _ = window.hide();
    } else {
        open(app, Open::default());
    }
}

pub fn open(app: &AppHandle, mut open: Open) {
    let Some(window) = app.get_webview_window(LABEL) else {
        return;
    };
    // Captured before the palette takes focus, so it describes the app the
    // user was in.
    open.context = ai::context(app);
    if let Ok(mut t) = OPENED_AT.lock() {
        *t = Some(Instant::now());
    }
    let _ = window.emit(OPEN_EVENT, open);
    place(&window);
    if let Err(err) = window.show().and_then(|_| window.set_focus()) {
        log::warn!("could not show the palette: {err}");
    }
    log::info!("palette opened");
}

/// Centered horizontally, a fifth of the way down, like Spotlight.
fn place(window: &tauri::WebviewWindow) {
    let monitor = window
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| window.primary_monitor().ok().flatten());
    let (Some(m), Ok(size)) = (monitor, window.outer_size()) else {
        let _ = window.center();
        return;
    };
    let (pos, screen) = (m.position(), m.size());
    let x = pos.x + (screen.width as i32 - size.width as i32) / 2;
    let y = pos.y + screen.height as i32 / 5;
    let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
}

pub fn hide(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.hide();
    }
}
