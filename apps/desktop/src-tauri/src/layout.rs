//! Window layouts per monitor setup (FR-SYS-04): while a setup stays the
//! same, where windows sit is saved every few minutes; when monitors are
//! plugged in and that setup has a saved layout, it is offered back (or
//! restored at once if the skill is set to Auto).

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use sidekick_core::Event;
use sidekick_layout::Placement;
use tauri::{AppHandle, Manager};

use crate::island::LABEL;
use crate::state::{AppState, lock};

pub const MONITOR_CONNECTED: &str = "system.monitor_connected";
const POLL: Duration = Duration::from_secs(15);
const SAVE_EVERY: Duration = Duration::from_secs(5 * 60);
/// A new setup must settle before its layout is saved.
const SETTLE: Duration = Duration::from_secs(2 * 60);

type Layouts = HashMap<String, Vec<Placement>>;

fn file(app: &AppHandle) -> Option<PathBuf> {
    app.state::<AppState>()
        .db_path
        .parent()
        .map(|d| d.join("layouts.json"))
}

fn load(app: &AppHandle) -> Layouts {
    file(app)
        .and_then(|f| std::fs::read_to_string(f).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save(app: &AppHandle, layouts: &Layouts) {
    if let (Some(f), Ok(json)) = (file(app), serde_json::to_string(layouts)) {
        let _ = std::fs::write(f, json);
    }
}

/// The current monitor setup as a stable key, and how many monitors.
fn setup(app: &AppHandle) -> Option<(String, usize)> {
    let window = app.get_webview_window(LABEL)?;
    let mut monitors: Vec<String> = window
        .available_monitors()
        .ok()?
        .iter()
        .map(|m| {
            let (p, s) = (m.position(), m.size());
            format!("{},{},{}x{}", p.x, p.y, s.width, s.height)
        })
        .collect();
    monitors.sort();
    Some((monitors.join(";"), monitors.len()))
}

fn allowed(app: &AppHandle) -> bool {
    let state = app.state::<AppState>();
    let s = lock(&state.settings);
    !s.pause.is_active(chrono::Utc::now()) && s.sensor_enabled("system")
}

pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut current: Option<(String, usize)> = None;
        let mut since = Instant::now();
        let mut saved_at: Option<Instant> = None;
        loop {
            tokio::time::sleep(POLL).await;
            let Some(now) = setup(&app) else { continue };
            if current.as_ref() != Some(&now) {
                let more = current.as_ref().is_some_and(|(_, n)| now.1 > *n);
                current = Some(now.clone());
                since = Instant::now();
                saved_at = None;
                if more && allowed(&app) && load(&app).get(&now.0).is_some_and(|l| !l.is_empty()) {
                    app.state::<AppState>().bus.publish(Event::new(
                        MONITOR_CONNECTED,
                        "system",
                        serde_json::json!({ "monitors": now.1 }),
                    ));
                }
                continue;
            }
            if since.elapsed() < SETTLE
                || saved_at.is_some_and(|t| t.elapsed() < SAVE_EVERY)
                || !allowed(&app)
            {
                continue;
            }
            saved_at = Some(Instant::now());
            let key = now.0.clone();
            let handle = app.clone();
            let _ = tauri::async_runtime::spawn_blocking(move || {
                let placements = sidekick_layout::capture();
                if placements.is_empty() {
                    return;
                }
                let mut layouts = load(&handle);
                layouts.insert(key, placements);
                save(&handle, &layouts);
            })
            .await;
        }
    });
}

/// Puts windows back where they were on this monitor setup.
pub async fn restore(app: &AppHandle) -> Result<String, String> {
    let (key, _) = setup(app).ok_or("no monitors found")?;
    let saved = load(app)
        .remove(&key)
        .ok_or("No layout saved for this monitor setup yet.")?;
    let moved = tauri::async_runtime::spawn_blocking(move || sidekick_layout::restore(&saved))
        .await
        .map_err(|e| e.to_string())?;
    Ok(format!("Moved {moved} windows back"))
}
