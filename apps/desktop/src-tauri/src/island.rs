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
pub const VISIBLE_EVENT: &str = "island://visible";
/// Whether a fullscreen app is in front on the island's screen, so the glance
/// can offer "Hide while fullscreen" right there.
pub const FULLSCREEN_EVENT: &str = "island://fullscreen";

/// About 40 Hz. The UI smooths it with springs, so this reads as continuous.
const CURSOR_POLL: Duration = Duration::from_millis(24);
/// Movements smaller than this (logical px) are not sent.
const CURSOR_MIN_DELTA: f64 = 1.0;
/// Extra room around an open panel that still takes clicks (logical px).
const ASK_MARGIN: f64 = 16.0;
/// How often the island claims the top of the z-order again.
const TOP_EVERY: Duration = Duration::from_secs(1);

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let window = app
        .get_webview_window(LABEL)
        .ok_or_else(|| tauri::Error::WindowNotFound)?;
    position_top_center(&window)?;
    // Show first: click-through needs a realized native window (on Linux,
    // setting it on a hidden window panics inside tao).
    window.show()?;
    window.set_ignore_cursor_events(true)?;
    spawn_top_keeper(window.clone());
    spawn_hover_tracker(app.clone(), window);
    Ok(())
}

fn position_top_center(window: &WebviewWindow) -> tauri::Result<()> {
    match window.primary_monitor()? {
        Some(m) => place_on(window, &m),
        None => Ok(()),
    }
}

fn place_on(window: &WebviewWindow, monitor: &tauri::Monitor) -> tauri::Result<()> {
    let screen = monitor.size();
    let origin = monitor.position();
    let size = window.outer_size()?;
    let x = origin.x + (screen.width as i32 - size.width as i32) / 2;
    window.set_position(PhysicalPosition::new(x, origin.y))
}

/// The monitor that holds point (x, y), in physical pixels.
fn monitor_at(monitors: &[tauri::Monitor], x: i64, y: i64) -> Option<&tauri::Monitor> {
    monitors.iter().find(|m| {
        let (p, s) = (m.position(), m.size());
        x >= i64::from(p.x)
            && x < i64::from(p.x) + i64::from(s.width)
            && y >= i64::from(p.y)
            && y < i64::from(p.y) + i64::from(s.height)
    })
}

/// Moves the island to the monitor of the window in front (FR-UI-06). Not
/// while Ask mode is open, and not for fullscreen windows (the island stays
/// put and visible on its own screen).
pub fn follow_active_monitor(app: &AppHandle, payload: &serde_json::Value) {
    if crate::ask::is_open(app) || payload["fullscreen"].as_bool().unwrap_or(false) {
        return;
    }
    let (Some(x), Some(y), Some(w), Some(h)) = (
        payload["x"].as_i64(),
        payload["y"].as_i64(),
        payload["width"].as_i64(),
        payload["height"].as_i64(),
    ) else {
        return;
    };
    let Some(window) = app.get_webview_window(LABEL) else {
        return;
    };
    let Ok(monitors) = window.available_monitors() else {
        return;
    };
    if monitors.len() < 2 {
        return;
    }
    let Some(target) = monitor_at(&monitors, x + w / 2, y + h / 2) else {
        return;
    };
    let current = window.current_monitor().ok().flatten();
    if current.is_some_and(|c| c.position() == target.position()) {
        return;
    }
    if let Err(err) = place_on(&window, target) {
        log::warn!("could not move the island: {err}");
    }
}

/// Puts the island back on top. Windows lets a window that goes
/// fullscreen (a browser after F11, a video player, slides) cover other
/// always-on-top windows; claiming the top again, without taking focus,
/// keeps the island above it. Exclusive-fullscreen games still win.
#[cfg(windows)]
pub fn keep_on_top(window: &WebviewWindow) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE, SetWindowPos,
    };
    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    // SAFETY: a valid window handle from Tauri; only the z-order changes.
    unsafe {
        SetWindowPos(
            hwnd.0 as _,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
        );
    }
}

#[cfg(not(windows))]
pub fn keep_on_top(window: &WebviewWindow) {
    let _ = window.set_always_on_top(true);
}

/// Keeps the island above other windows: right away when the window in
/// front changes, and every second in case something climbed over it.
fn spawn_top_keeper(window: WebviewWindow) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(TOP_EVERY).await;
            keep_on_top(&window);
        }
    });
}

/// Optionally fades the island while a fullscreen app (a game, a video, a
/// slideshow) is in front (FR-UI-09). Off by default: the island stays on
/// top of everything.
pub fn follow_fullscreen(app: &AppHandle, payload: &serde_json::Value) {
    let Some(window) = app.get_webview_window(LABEL) else {
        return;
    };
    keep_on_top(&window);
    let wanted = lock(&app.state::<AppState>().settings).hide_in_fullscreen;
    // The sensor decides what is fullscreen (an exact monitor match, so
    // maximized windows never count). Only for the island's own monitor.
    let here = payload["fullscreen"].as_bool().unwrap_or(false)
        && window
            .current_monitor()
            .ok()
            .flatten()
            .is_some_and(|monitor| {
                let (pos, size) = (monitor.position(), monitor.size());
                let m = &payload["monitor"];
                m["x"].as_i64() == Some(i64::from(pos.x))
                    && m["y"].as_i64() == Some(i64::from(pos.y))
                    && m["width"].as_i64() == Some(i64::from(size.width))
                    && m["height"].as_i64() == Some(i64::from(size.height))
            });
    static LAST_HERE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if LAST_HERE.swap(here, std::sync::atomic::Ordering::Relaxed) != here {
        let _ = app.emit_to(LABEL, FULLSCREEN_EVENT, here);
    }
    let fullscreen = wanted && here;
    let state = app.state::<AppState>();
    let mut hidden = lock(&state.island_hidden);
    if fullscreen == *hidden {
        return;
    }
    *hidden = fullscreen;
    // The native window stays shown: hiding and showing it again on Windows
    // activates it (stealing focus) and can drop it in the z-order. The UI
    // fades out instead, and the hover tracker keeps it click-through.
    if fullscreen {
        let _ = window.set_ignore_cursor_events(true);
    }
    let _ = app.emit_to(LABEL, VISIBLE_EVENT, !fullscreen);
}

/// Polls the global cursor. Streams its position to the UI (the mascot's eyes
/// follow it anywhere on screen) and toggles click-through when it enters or
/// leaves the interactive rect. The webview gets no mouse events while
/// click-through is on, so hover is reported from here too.
fn spawn_hover_tracker(app: AppHandle, window: WebviewWindow) {
    tauri::async_runtime::spawn(async move {
        let mut inside = false;
        // Click-through as last applied, with what it was worked out from:
        // re-applied whenever any of them changes.
        let mut applied: Option<(bool, bool, bool)> = None;
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

            let state = app.state::<AppState>();
            let asking = crate::ask::is_open(&app);
            let hidden = *lock(&state.island_hidden);
            let rect = *lock(&state.hit_rect);
            // Only the panel itself takes clicks, so apps around it stay
            // usable even while Ask or the welcome is open. While a panel is
            // open its rect can lag a growing animation, hence the margin.
            let pad = if asking { ASK_MARGIN } else { 0.0 };
            // Hidden in fullscreen, the island still comes back under the
            // cursor (so its switch is reachable); elsewhere it lets clicks by.
            let now_through = !rect.contains_within(pos.x, pos.y, pad);
            if applied != Some((now_through, asking, hidden)) {
                applied = Some((now_through, asking, hidden));
                if let Err(err) = window.set_ignore_cursor_events(now_through) {
                    log::warn!("could not toggle click-through: {err}");
                }
            }
            let now_inside = rect.contains(pos.x, pos.y);
            if now_inside == inside {
                continue;
            }
            inside = now_inside;
            state
                .hovered
                .store(inside, std::sync::atomic::Ordering::Relaxed);
            let _ = app.emit_to(LABEL, HOVER_EVENT, inside);
            // Hide parks welcome; hovering the compact island brings it back.
            if inside {
                crate::ask::on_island_hover(&app);
            }
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
