//! The command palette window (FR-UI-07): a global shortcut toggles it, and
//! it hides again when it loses focus.

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, WindowEvent};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::ai;

pub const LABEL: &str = "palette";
pub const OPEN_EVENT: &str = "palette://open";

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
                let _ = w.hide();
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
    .map_err(|e| format!("{hotkey} could not be registered (another app may use it): {e}"))
}

pub fn toggle(app: &AppHandle) {
    let Some(window) = app.get_webview_window(LABEL) else {
        return;
    };
    if window.is_visible().unwrap_or(false) && window.is_focused().unwrap_or(false) {
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
    let _ = window.emit(OPEN_EVENT, open);
    let _ = window.center();
    let _ = window.show();
    let _ = window.set_focus();
}

pub fn hide(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.hide();
    }
}
