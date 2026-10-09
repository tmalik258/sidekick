use std::time::Duration;

use sidekick_core::{Event, EventBus, Sensitivity};
use tokio::task::JoinHandle;

use crate::{Sensor, SensorGate};

/// Notices which app and window the user is in.
pub struct WindowSensor;

impl WindowSensor {
    pub const ID: &'static str = "window";
    pub const EVENT_KIND: &'static str = "window.focused";
}

const CHECK_EVERY: Duration = Duration::from_millis(500);

/// Windows that pass over the screen for a moment (the screenshot overlay,
/// Notification Center): not the app the user is in, and never fullscreen.
const PASSING: &[&str] = &[
    "screenclippinghost.exe",
    "snippingtool.exe",
    "screensketch.exe",
    "shellexperiencehost.exe",
];

fn passing(exe: &str) -> bool {
    PASSING.iter().any(|p| exe.eq_ignore_ascii_case(p))
}

impl Sensor for WindowSensor {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn spawn(self: Box<Self>, bus: EventBus, gate: SensorGate) -> JoinHandle<()> {
        tokio::spawn(async move {
            let own_pid = u64::from(std::process::id());
            let mut last: Option<(String, String, Option<Rect>)> = None;
            let mut last_raw: Option<Rect> = None;
            let mut tick = tokio::time::interval(CHECK_EVERY);
            loop {
                tick.tick().await;
                let Ok(Ok(win)) =
                    tokio::task::spawn_blocking(active_win_pos_rs::get_active_window).await
                else {
                    continue;
                };
                let passing_by = win
                    .process_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(passing);
                if win.process_id == own_pid || passing_by {
                    continue;
                }
                // Fullscreen is part of the key: pressing F11 or Esc changes
                // nothing else about the window.
                let raw = tokio::task::spawn_blocking(fullscreen_monitor)
                    .await
                    .unwrap_or_default();
                // Only trust fullscreen once it holds for two checks, so a
                // passing overlay never hides the island.
                let fullscreen = raw.filter(|_| raw == last_raw);
                last_raw = raw;
                let is_shell = win
                    .process_path
                    .file_name()
                    .is_some_and(|n| n.eq_ignore_ascii_case("explorer.exe"));
                let fullscreen = fullscreen.filter(|_| !is_shell);
                let key = (win.app_name.clone(), win.title.clone(), fullscreen);
                if !gate.allows(Self::ID) {
                    // Report the window again when resumed.
                    last = None;
                    continue;
                }
                if last.as_ref() == Some(&key) {
                    continue;
                }
                last = Some(key);
                let exe = win
                    .process_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                bus.publish(
                    Event::new(
                        Self::EVENT_KIND,
                        Self::ID,
                        serde_json::json!({
                            "app": win.app_name,
                            "exe": exe,
                            "path": win.process_path.to_string_lossy(),
                            "title": win.title,
                            "pid": win.process_id,
                            "x": win.position.x,
                            "y": win.position.y,
                            "width": win.position.width,
                            "height": win.position.height,
                            "fullscreen": fullscreen.is_some(),
                            "monitor": fullscreen,
                        }),
                    )
                    .with_sensitivity(Sensitivity::Personal),
                );
            }
        })
    }
}

/// A rectangle in physical screen pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// The monitor the foreground window fills, if it is truly fullscreen (a
/// video, a game, a browser after F11, slides). A maximized window is not:
/// Windows reports it 8px past every monitor edge (its invisible resize
/// borders), so only an exact match with the monitor counts.
#[cfg(windows)]
const SHELL_CLASSES: &[&str] = &[
    "WorkerW",
    "Progman",
    "XamlExplorerHostIslandWindow",
    "MultitaskingViewFrame",
    "TaskSwitcherWnd",
    "ForegroundStaging",
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
    "Windows.UI.Core.CoreWindow",
];

#[cfg(windows)]
fn fullscreen_monitor() -> Option<Rect> {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONULL, MONITORINFO, MonitorFromWindow,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetForegroundWindow, GetShellWindow, GetWindowRect,
    };

    // SAFETY: plain Win32 queries on the foreground window with properly
    // sized out-parameters; nothing is retained after the calls.
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() || hwnd == GetShellWindow() {
            return None;
        }
        // The desktop, the Alt+Tab and Win+Tab switchers, Start and the
        // taskbar fill the screen too, but they are the shell, not an app.
        let mut class = [0u16; 64];
        let len = GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32);
        let class = String::from_utf16_lossy(&class[..len.max(0) as usize]);
        if SHELL_CLASSES.contains(&class.as_str()) {
            return None;
        }
        let mut rect: RECT = std::mem::zeroed();
        if GetWindowRect(hwnd, &mut rect) == 0 {
            return None;
        }
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONULL);
        if monitor.is_null() {
            return None;
        }
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(monitor, &mut info) == 0 {
            return None;
        }
        let m = info.rcMonitor;
        let exact = rect.left == m.left
            && rect.top == m.top
            && rect.right == m.right
            && rect.bottom == m.bottom;
        exact.then_some(Rect {
            x: m.left,
            y: m.top,
            width: m.right - m.left,
            height: m.bottom - m.top,
        })
    }
}

#[cfg(not(windows))]
fn fullscreen_monitor() -> Option<Rect> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screenshot_overlay_is_not_an_app() {
        assert!(passing("ScreenClippingHost.exe"));
        assert!(passing("SnippingTool.exe"));
        assert!(passing("ScreenSketch.exe"));
        assert!(!passing("notepad.exe"));
    }
}
