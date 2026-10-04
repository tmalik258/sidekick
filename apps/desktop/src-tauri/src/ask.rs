//! Ask mode: the island itself grows into a panel for commands
//! and chat. A global shortcut (Ctrl+Space by default) or the island's Ask
//! button opens it; Esc or clicking anywhere else closes it. Until onboarding
//! is finished, the island stays locked on welcome: blur, Esc and the Ask
//! shortcut cannot dismiss it for good. Hide parks it (other apps stay usable);
//! hovering the island brings welcome back. Only Skip or Start mark onboarded.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, WindowEvent};
use tauri_plugin_global_shortcut::Shortcut;

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
/// Welcome stays put until Skip/Start; normal Ask still closes on blur.
static KEEP_ON_BLUR: AtomicBool = AtomicBool::new(false);
/// User hid welcome for now; hover (or Ask shortcut) brings it back.
static WELCOME_DEFERRED: AtomicBool = AtomicBool::new(false);

/// Why Ask folded away. `defer` parks welcome; `close` is a real dismiss.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Close {
    pub reason: &'static str,
}

const CLOSE_REAL: Close = Close { reason: "close" };
const CLOSE_DEFER: Close = Close { reason: "defer" };

fn just_opened() -> bool {
    OPENED_AT
        .lock()
        .ok()
        .and_then(|t| *t)
        .is_some_and(|t| t.elapsed() < BLUR_GRACE)
}

fn keep_on_blur() -> bool {
    KEEP_ON_BLUR.load(Ordering::SeqCst)
}

fn is_deferred() -> bool {
    WELCOME_DEFERRED.load(Ordering::SeqCst)
}

/// Welcome no longer needs to fight blur once onboarding is marked done
/// (Skip, Start, or jumping into a Settings tab from the checklist).
pub fn release_sticky() {
    KEEP_ON_BLUR.store(false, Ordering::SeqCst);
    WELCOME_DEFERRED.store(false, Ordering::SeqCst);
}

fn sticky_welcome(view: Option<&str>) -> bool {
    view == Some("welcome")
}

fn needs_welcome(app: &AppHandle) -> bool {
    !lock(&app.state::<AppState>().settings).onboarded
}

/// Sent to the island each time ask mode opens.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Open {
    pub context: ai::Context,
    /// Text to put in the input, e.g. "Explain this error".
    pub prompt: Option<String>,
    /// Send `prompt` right away.
    pub ask: bool,
    /// Attach the clipboard to that first question.
    pub clipboard: bool,
    /// Text of the web page the question is about (from the extension).
    pub page: Option<String>,
    /// "settings" opens the Settings panel instead of Ask.
    pub view: Option<&'static str>,
    /// A tool to start with: "screen" asks about the screen, "clipboard"
    /// shows clipboard history.
    pub tool: Option<&'static str>,
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
                if keep_on_blur() || (needs_welcome(&app) && !is_deferred()) {
                    // The welcome stays on screen until Skip, Start or Hide,
                    // but never takes focus back: Connect opens the browser,
                    // and the user must be able to use it (and any other
                    // app). Clicking the welcome again carries on.
                } else if just_opened() {
                    // The shortcut that opened Ask can blur it for a moment.
                    let _ = w.set_focus();
                } else {
                    let app = app.clone();
                    let w = w.clone();
                    tauri::async_runtime::spawn(async move {
                        tokio::time::sleep(BLUR_SETTLE).await;
                        if is_deferred() {
                            return;
                        }
                        if keep_on_blur() || needs_welcome(&app) {
                            return;
                        }
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

/// Checks the Ask shortcut, then registers every shortcut again. Returns an
/// error the settings UI can show when the Ask one cannot be used.
pub fn register(app: &AppHandle, hotkey: &str) -> Result<(), String> {
    hotkey
        .parse::<Shortcut>()
        .map_err(|e| format!("{hotkey} is not a valid shortcut: {e}"))?;
    let failed = crate::shortcuts::register_all(app);
    if let Some(f) = failed.iter().find(|f| f.starts_with(hotkey)) {
        return Err(format!("{f}; pick another"));
    }
    Ok(())
}

pub fn toggle(app: &AppHandle) {
    if needs_welcome(app) {
        resume_welcome(app);
        return;
    }
    if is_open(app) {
        close(app);
    } else {
        open(app, Open::default());
    }
}

/// Parks welcome without finishing onboarding. Other apps stay usable until
/// the cursor enters the island again (or Ask is pressed).
pub fn defer_welcome(app: &AppHandle) {
    if !needs_welcome(app) {
        close(app);
        return;
    }
    WELCOME_DEFERRED.store(true, Ordering::SeqCst);
    KEEP_ON_BLUR.store(false, Ordering::SeqCst);
    let state = app.state::<AppState>();
    if !state.ask_open.swap(false, Ordering::SeqCst) {
        return;
    }
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.emit(CLOSE_EVENT, CLOSE_DEFER);
        if *lock(&state.island_hidden) {
            let _ = window.emit(island::VISIBLE_EVENT, false);
        }
    }
    suggestions::resume(app);
}

/// Clears a Hide and shows welcome again (island hover or Ask shortcut).
pub fn resume_welcome(app: &AppHandle) {
    if !needs_welcome(app) {
        return;
    }
    WELCOME_DEFERRED.store(false, Ordering::SeqCst);
    ensure_welcome(app);
}

/// Hover tracker: resume parked welcome, or repair a Locked session that never
/// reached the webview (missed ask://open).
pub fn on_island_hover(app: &AppHandle) {
    if !needs_welcome(app) {
        return;
    }
    if is_deferred() {
        resume_welcome(app);
    } else if !is_open(app) {
        ensure_welcome(app);
    }
}

/// Opens welcome if onboarding is unfinished and not parked by Hide.
/// Always re-emits so a missed cold-start event is repaired (Locked state).
pub fn ensure_welcome(app: &AppHandle) {
    if !needs_welcome(app) || is_deferred() {
        return;
    }
    // Voice gate: do not show welcome until the voice can speak.
    if !crate::voice::voice_ready(app) {
        crate::voice::prepare_then_welcome(app);
        return;
    }
    open(
        app,
        Open {
            view: Some("welcome"),
            ..Default::default()
        },
    );
}

pub fn open(app: &AppHandle, mut open: Open) {
    let Some(window) = app.get_webview_window(LABEL) else {
        return;
    };
    // Captured before the island takes focus, so it describes the app the
    // user was in.
    open.context = ai::context(app);
    if needs_welcome(app) {
        open.view = Some("welcome");
        WELCOME_DEFERRED.store(false, Ordering::SeqCst);
    }
    if let Ok(mut t) = OPENED_AT.lock() {
        *t = Some(Instant::now());
    }
    KEEP_ON_BLUR.store(sticky_welcome(open.view), Ordering::SeqCst);
    let state = app.state::<AppState>();
    state.ask_open.store(true, Ordering::SeqCst);
    // Click-through is the hover tracker's job: only the panel takes clicks.
    if *lock(&state.island_hidden) {
        let _ = window.emit(island::VISIBLE_EVENT, true);
    }
    let welcome = sticky_welcome(open.view);
    let _ = window.emit(OPEN_EVENT, open);
    if welcome {
        crate::voice::welcome_greet(app);
    }
    if let Err(err) = window.set_focus() {
        log::warn!("could not focus the island: {err}");
    }
}

pub fn close(app: &AppHandle) {
    if needs_welcome(app) && !is_deferred() {
        // Skip/Start must set onboarded before close; otherwise reopen welcome.
        ensure_welcome(app);
        return;
    }
    let state = app.state::<AppState>();
    if !state.ask_open.swap(false, Ordering::SeqCst) {
        return;
    }
    KEEP_ON_BLUR.store(false, Ordering::SeqCst);
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.emit(CLOSE_EVENT, CLOSE_REAL);
        if *lock(&state.island_hidden) {
            let _ = window.emit(island::VISIBLE_EVENT, false);
        }
    }
    // Suggestions that waited while the user was typing can show now.
    suggestions::resume(app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_welcome_is_sticky_on_blur() {
        assert!(sticky_welcome(Some("welcome")));
        assert!(!sticky_welcome(Some("settings")));
        assert!(!sticky_welcome(None));
    }
}
