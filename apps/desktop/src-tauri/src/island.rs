//! The island window: always on top, centered at the top edge of the primary
//! monitor, click-through everywhere except the part the UI reports as
//! interactive (FR-UI-01).

use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, WebviewWindow};

use crate::state::{AppState, lock};

pub const LABEL: &str = "island";
pub const HOVER_EVENT: &str = "island://hover";

const HOVER_POLL: Duration = Duration::from_millis(40);

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let window = app
        .get_webview_window(LABEL)
        .ok_or_else(|| tauri::Error::WindowNotFound)?;
    position_top_center(&window)?;
    // Show first: click-through needs a realized native window (on Linux,
    // setting it on a hidden window panics inside tao).
    window.show()?;
    window.set_ignore_cursor_events(true)?;
    spawn_hover_tracker(app.clone(), window);
    Ok(())
}

fn position_top_center(window: &WebviewWindow) -> tauri::Result<()> {
    let monitor = match window.primary_monitor()? {
        Some(m) => m,
        None => return Ok(()),
    };
    let screen = monitor.size();
    let origin = monitor.position();
    let size = window.outer_size()?;
    let x = origin.x + (screen.width as i32 - size.width as i32) / 2;
    window.set_position(PhysicalPosition::new(x, origin.y))
}

/// Polls the cursor and toggles click-through when it enters or leaves the
/// interactive rect. The webview gets no mouse events while click-through is
/// on, so hover is reported to the UI from here.
fn spawn_hover_tracker(app: AppHandle, window: WebviewWindow) {
    tauri::async_runtime::spawn(async move {
        let mut inside = false;
        loop {
            tokio::time::sleep(HOVER_POLL).await;
            let now_inside = match cursor_in_hit_rect(&app, &window) {
                Some(v) => v,
                None => continue,
            };
            if now_inside == inside {
                continue;
            }
            inside = now_inside;
            if let Err(err) = window.set_ignore_cursor_events(!inside) {
                log::warn!("could not toggle click-through: {err}");
            }
            let _ = app.emit_to(LABEL, HOVER_EVENT, inside);
        }
    });
}

fn cursor_in_hit_rect(app: &AppHandle, window: &WebviewWindow) -> Option<bool> {
    let cursor = app.cursor_position().ok()?;
    let origin = window.outer_position().ok()?;
    let scale = window.scale_factor().ok()?;
    let x = (cursor.x - f64::from(origin.x)) / scale;
    let y = (cursor.y - f64::from(origin.y)) / scale;
    let rect = *lock(&app.state::<AppState>().hit_rect);
    Some(rect.contains(x, y))
}
