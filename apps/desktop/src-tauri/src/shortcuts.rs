//! Global shortcuts: Ask, plus talk, accept, dismiss, screen, clipboard
//! history, pause and settings. All are registered here, so changing one
//! re-registers the lot in one place.

use chrono::Utc;
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::ask;
use crate::state::{AppState, lock};

/// Registers every shortcut from settings. Returns the ones that failed
/// (another app may own them); the rest still work.
pub fn register_all(app: &AppHandle) -> Vec<String> {
    let settings = lock(&app.state::<AppState>().settings).clone();
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    // unregister_all also dropped the Alt+N keys of a suggestion on screen.
    crate::suggestions::rebind_keys(app);
    let mut failed = Vec::new();
    let mut wanted: Vec<(String, String)> = vec![("ask".into(), settings.palette_hotkey.clone())];
    wanted.extend(
        settings
            .shortcuts
            .iter()
            .map(|(a, k)| (a.clone(), k.clone())),
    );
    for (action, keys) in wanted {
        if keys.trim().is_empty() {
            continue;
        }
        let Ok(shortcut) = keys.parse::<Shortcut>() else {
            failed.push(format!("{keys} ({action}) is not a valid shortcut"));
            continue;
        };
        let result = gs.on_shortcut(shortcut, move |app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                let app = app.clone();
                let action = action.clone();
                // Off the handler's thread: some actions re-register keys.
                tauri::async_runtime::spawn(async move { run(&app, &action) });
            }
        });
        if result.is_err() {
            failed.push(format!("{keys} is used by another app"));
        }
    }
    for f in &failed {
        log::warn!("shortcut: {f}");
    }
    failed
}

/// Checks a shortcut parses, for the settings recorder.
pub fn valid(keys: &str) -> bool {
    keys.trim().is_empty() || keys.parse::<Shortcut>().is_ok()
}

fn run(app: &AppHandle, action: &str) {
    log::info!("shortcut: {action}");
    match action {
        "ask" => ask::toggle(app),
        "talk" => {
            if !ask::is_open(app) {
                ask::open(app, ask::Open::default());
            }
            if let Err(err) = crate::voice::listen(app) {
                log::info!("talk shortcut: {err}");
            }
        }
        "accept" => {
            if let Some(s) = crate::suggestions::current(app) {
                let _ = crate::suggestions::choose(app, &s.id, 0);
            }
        }
        "dismiss" => {
            // Stop comes first: a task in progress ends between steps.
            if crate::ai::cancel_all(app) {
                log::info!("stopped the running task");
                return;
            }
            if let Some(s) = crate::suggestions::current(app) {
                let _ = crate::suggestions::dismiss(app, &s.id, "shortcut");
            }
        }
        "screen" => ask::open(
            app,
            ask::Open {
                tool: Some("screen"),
                ..Default::default()
            },
        ),
        "clipboard" => ask::open(
            app,
            ask::Open {
                tool: Some("clipboard"),
                ..Default::default()
            },
        ),
        "settings" => ask::open(
            app,
            ask::Open {
                view: Some("settings"),
                ..Default::default()
            },
        ),
        "focus" => {
            if crate::focus::active() {
                crate::focus::stop(app);
            } else {
                crate::focus::start(app, crate::focus::DEFAULT_MINUTES);
            }
        }
        "pause" => {
            let paused = lock(&app.state::<AppState>().settings)
                .pause
                .is_active(Utc::now());
            let pause = if paused {
                sidekick_core::Pause::None
            } else {
                sidekick_core::Pause::for_minutes(60, Utc::now())
            };
            let _ = crate::commands::set_pause(app, pause);
        }
        _ => {}
    }
}
