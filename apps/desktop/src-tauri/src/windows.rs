use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

const SETTINGS_LABEL: &str = "settings";

/// Opens the settings window, or focuses it when it is already open.
pub fn open_settings(app: &AppHandle) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(SETTINGS_LABEL) {
        window.unminimize()?;
        window.show()?;
        return window.set_focus();
    }
    WebviewWindowBuilder::new(app, SETTINGS_LABEL, WebviewUrl::App("settings/".into()))
        .title("Sidekick Settings")
        .inner_size(640.0, 760.0)
        .min_inner_size(480.0, 520.0)
        .center()
        .build()?;
    Ok(())
}
