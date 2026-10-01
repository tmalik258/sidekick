use tauri::AppHandle;

/// Opens Settings inside the island (there is no separate settings window).
pub fn open_settings(app: &AppHandle) -> tauri::Result<()> {
    crate::ask::open(
        app,
        crate::ask::Open {
            view: Some("settings"),
            ..Default::default()
        },
    );
    Ok(())
}
