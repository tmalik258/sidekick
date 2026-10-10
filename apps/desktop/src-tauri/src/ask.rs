//! Ask mode: the island itself grows into a panel for commands
//! and chat. Ctrl+Space opens Ask; Ctrl+Shift+Space opens Agents. Esc or
//! clicking anywhere else closes it. Until onboarding is finished, the island
//! stays locked on welcome: blur, Esc and the Ask shortcut cannot dismiss it
//! for good. Hide parks it (other apps stay usable); hovering the island
//! brings welcome back (the UI asks, unless a waiting guide is showing).
//! Only Skip or Start mark onboarded.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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
/// Serializes open/close/toggle so duplicate shortcut events cannot race.
static PANEL_LOCK: Mutex<()> = Mutex::new(());
/// Bumped on each open/close; blur-settle tasks only run if their gen still matches.
static BLUR_GEN: AtomicU64 = AtomicU64::new(0);
/// Welcome stays put until Skip/Start; normal Ask still closes on blur.
static KEEP_ON_BLUR: AtomicBool = AtomicBool::new(false);
/// User hid welcome for now; hover (or Ask shortcut) brings it back.
static WELCOME_DEFERRED: AtomicBool = AtomicBool::new(false);
/// Last panel tab the open shortcuts asked for ("ask" / "agents").
static PANEL_TAB: Mutex<Option<&'static str>> = Mutex::new(None);
/// Monotonic id for each open; close carries the same id so the UI can drop
/// stale events when shortcuts fire faster than the webview applies them.
static PANEL_GEN: AtomicU64 = AtomicU64::new(0);

/// Why Ask folded away. `defer` parks welcome; `close` is a real dismiss.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Close {
    pub reason: &'static str,
    pub panel_gen: u64,
}

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
    /// Opens Agents with a new session in this project.
    pub project: Option<String>,
    /// Which Settings tab to show with view "settings" ("memory").
    pub settings_tab: Option<&'static str>,
    /// Ask panel tab to show: "ask" or "agents".
    pub tab: Option<&'static str>,
    /// When the open was asked for (ms since 1970), for the open-to-ready
    /// timing.
    pub sent_at: i64,
    /// Matches the close event that ends this open.
    pub panel_gen: u64,
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
                    let blur_gen = BLUR_GEN.load(Ordering::SeqCst);
                    tauri::async_runtime::spawn(async move {
                        tokio::time::sleep(BLUR_SETTLE).await;
                        if BLUR_GEN.load(Ordering::SeqCst) != blur_gen {
                            return;
                        }
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
    toggle_tab(app, "ask");
}

/// Opens (or closes, if already on) the Agents tab.
pub fn toggle_agents(app: &AppHandle) {
    toggle_tab(app, "agents");
}

fn panel_tab() -> Option<&'static str> {
    PANEL_TAB.lock().ok().and_then(|t| *t)
}

fn toggle_tab(app: &AppHandle, tab: &'static str) {
    let Ok(_guard) = PANEL_LOCK.lock() else {
        return;
    };
    let open_now = is_open(app);
    let current = panel_tab();
    if needs_welcome(app) {
        log::info!("welcome resumed (was parked: {})", is_deferred());
        WELCOME_DEFERRED.store(false, Ordering::SeqCst);
        ensure_welcome_impl(app);
        return;
    }
    if open_now && current == Some(tab) {
        close_impl(app);
        return;
    }
    open_impl(
        app,
        Open {
            tab: Some(tab),
            ..Default::default()
        },
    );
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
    let was_open = state.ask_open.swap(false, Ordering::SeqCst);
    log::info!("welcome parked (was open: {was_open})");
    if !was_open {
        return;
    }
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.emit(
            CLOSE_EVENT,
            Close {
                reason: "defer",
                panel_gen: PANEL_GEN.load(Ordering::SeqCst),
            },
        );
        if *lock(&state.island_hidden) {
            let _ = window.emit(island::VISIBLE_EVENT, false);
        }
    }
    suggestions::resume(app);
}

/// Clears a Hide and shows welcome again (island hover or Ask shortcut).
/// Re-emits when already open, so a missed cold-start ask://open is repaired.
pub fn resume_welcome(app: &AppHandle) {
    if !needs_welcome(app) {
        return;
    }
    log::info!("welcome resumed (was parked: {})", is_deferred());
    WELCOME_DEFERRED.store(false, Ordering::SeqCst);
    let Ok(_guard) = PANEL_LOCK.lock() else {
        return;
    };
    ensure_welcome_impl(app);
}

/// Opens welcome if onboarding is unfinished and not parked by Hide.
/// Always re-emits so a missed cold-start event is repaired (Locked state).
pub fn ensure_welcome(app: &AppHandle) {
    let Ok(_guard) = PANEL_LOCK.lock() else {
        return;
    };
    ensure_welcome_impl(app);
}

fn ensure_welcome_impl(app: &AppHandle) {
    if !needs_welcome(app) || is_deferred() {
        return;
    }
    // Voice gate: do not show welcome until the voice can speak.
    if !crate::voice::voice_ready(app) {
        crate::voice::prepare_then_welcome(app);
        return;
    }
    open_impl(
        app,
        Open {
            view: Some("welcome"),
            ..Default::default()
        },
    );
}

pub fn open(app: &AppHandle, open: Open) {
    let Ok(_guard) = PANEL_LOCK.lock() else {
        return;
    };
    open_impl(app, open);
}

fn open_impl(app: &AppHandle, mut open: Open) {
    BLUR_GEN.fetch_add(1, Ordering::SeqCst);
    crate::instant::refresh(app);
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
    open.sent_at = chrono::Utc::now().timestamp_millis();
    open.panel_gen = PANEL_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    if open.project.is_some() {
        open.tab = Some("agents");
    }
    if let Ok(mut t) = PANEL_TAB.lock() {
        *t = open.tab.or(Some("ask"));
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
    } else {
        ai::warm_up(app);
    }
    if let Err(err) = window.set_focus() {
        log::warn!("could not focus the island: {err}");
    }
}

pub fn close(app: &AppHandle) {
    let Ok(_guard) = PANEL_LOCK.lock() else {
        return;
    };
    close_impl(app);
}

fn close_impl(app: &AppHandle) {
    if needs_welcome(app) && !is_deferred() {
        // Skip/Start must set onboarded before close; otherwise reopen welcome.
        ensure_welcome_impl(app);
        return;
    }
    let state = app.state::<AppState>();
    if !state.ask_open.swap(false, Ordering::SeqCst) {
        return;
    }
    BLUR_GEN.fetch_add(1, Ordering::SeqCst);
    if let Ok(mut t) = PANEL_TAB.lock() {
        *t = None;
    }
    KEEP_ON_BLUR.store(false, Ordering::SeqCst);
    if let Some(window) = app.get_webview_window(LABEL) {
        let panel_gen = PANEL_GEN.load(Ordering::SeqCst);
        let _ = window.emit(
            CLOSE_EVENT,
            Close {
                reason: "close",
                panel_gen,
            },
        );
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
