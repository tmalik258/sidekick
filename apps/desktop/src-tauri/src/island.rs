//! The island window: always on top, centered at the top edge of the primary
//! monitor, click-through everywhere except the part the UI reports as
//! interactive (FR-UI-01).

use std::time::Duration;

use serde::Serialize;

use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, WebviewWindow};

use crate::state::{AppState, lock};

pub const LABEL: &str = "island";
pub const HOVER_EVENT: &str = "island://hover";
pub const CURSOR_EVENT: &str = "island://cursor";

/// About 40 Hz. The UI smooths it with springs, so this reads as continuous.
const CURSOR_POLL: Duration = Duration::from_millis(24);
/// Movements smaller than this (logical px) are not sent.
const CURSOR_MIN_DELTA: f64 = 1.0;

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

/// Hides the island while a fullscreen app (a game, a video, a slideshow) is
/// in front, and brings it back afterwards (FR-UI-09).
pub fn follow_fullscreen(app: &AppHandle, payload: &serde_json::Value) {
    let Some(window) = app.get_webview_window(LABEL) else {
        return;
    };
    let exe = payload["exe"].as_str().unwrap_or_default();
    let fullscreen = match (
        payload["width"].as_f64(),
        payload["height"].as_f64(),
        window.current_monitor().ok().flatten(),
    ) {
        // The desktop itself reports a screen-sized window; it is not fullscreen.
        (Some(w), Some(h), Some(monitor)) if exe != "explorer.exe" => {
            let screen = monitor.size();
            w >= f64::from(screen.width) - 1.0 && h >= f64::from(screen.height) - 1.0
        }
        _ => false,
    };
    let state = app.state::<AppState>();
    let mut hidden = lock(&state.island_hidden);
    if fullscreen == *hidden {
        return;
    }
    *hidden = fullscreen;
    let result = if fullscreen {
        window.hide()
    } else {
        window.show()
    };
    if let Err(err) = result {
        log::warn!("could not toggle island for fullscreen: {err}");
    }
}

/// Polls the global cursor. Streams its position to the UI (the mascot's eyes
/// follow it anywhere on screen) and toggles click-through when it enters or
/// leaves the interactive rect. The webview gets no mouse events while
/// click-through is on, so hover is reported from here too.
fn spawn_hover_tracker(app: AppHandle, window: WebviewWindow) {
    tauri::async_runtime::spawn(async move {
        let mut inside = false;
        let mut last: Option<CursorPos> = None;
        loop {
            tokio::time::sleep(CURSOR_POLL).await;
            let Some(pos) = cursor_in_window(&app, &window) else {
                continue;
            };
            if last.is_none_or(|l| l.moved_from(pos)) {
                last = Some(pos);
                let _ = app.emit_to(LABEL, CURSOR_EVENT, pos);
            }

            let now_inside = lock(&app.state::<AppState>().hit_rect).contains(pos.x, pos.y);
            if now_inside == inside {
                continue;
            }
            inside = now_inside;
            app.state::<AppState>()
                .hovered
                .store(inside, std::sync::atomic::Ordering::Relaxed);
            if let Err(err) = window.set_ignore_cursor_events(!inside) {
                log::warn!("could not toggle click-through: {err}");
            }
            let _ = app.emit_to(LABEL, HOVER_EVENT, inside);
        }
    });
}

/// Cursor position in logical pixels relative to the window's top-left corner.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct CursorPos {
    pub x: f64,
    pub y: f64,
}

impl CursorPos {
    /// True when the cursor moved enough to be worth telling the UI.
    fn moved_from(self, other: CursorPos) -> bool {
        (self.x - other.x).abs() >= CURSOR_MIN_DELTA || (self.y - other.y).abs() >= CURSOR_MIN_DELTA
    }
}

fn cursor_in_window(app: &AppHandle, window: &WebviewWindow) -> Option<CursorPos> {
    let cursor = app.cursor_position().ok()?;
    let origin = window.outer_position().ok()?;
    let scale = window.scale_factor().ok()?;
    Some(CursorPos {
        x: (cursor.x - f64::from(origin.x)) / scale,
        y: (cursor.y - f64::from(origin.y)) / scale,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignores_sub_pixel_jitter() {
        let a = CursorPos { x: 10.0, y: 10.0 };
        assert!(!a.moved_from(CursorPos { x: 10.4, y: 9.6 }));
        assert!(a.moved_from(CursorPos { x: 11.0, y: 10.0 }));
    }
}
