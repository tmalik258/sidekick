use sidekick_core::Pause;
use tauri::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Wry};

use crate::{commands, suggestions, windows};

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let menu = Menu::with_items(
        app,
        &[
            &item(app, "focus_25", "Focus 25 minutes")?,
            &item(app, "focus_end", "End focus")?,
            &PredefinedMenuItem::separator(app)?,
            &item(app, "pause_15", "Pause 15 minutes")?,
            &item(app, "pause_60", "Pause 1 hour")?,
            &item(app, "pause_forever", "Pause until resumed")?,
            &item(app, "resume", "Resume")?,
            &PredefinedMenuItem::separator(app)?,
            &item(app, "demo", "Run demo suggestion")?,
            &item(app, "settings", "Settings")?,
            &PredefinedMenuItem::separator(app)?,
            &item(app, "quit", "Quit Sidekick")?,
        ],
    )?;

    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("Sidekick")
        .menu(&menu)
        .on_menu_event(on_menu_event);
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

fn item(app: &AppHandle, id: &str, text: &str) -> tauri::Result<MenuItem<Wry>> {
    MenuItem::with_id(app, id, text, true, None::<&str>)
}

fn on_menu_event(app: &AppHandle, event: MenuEvent) {
    let result = match event.id().as_ref() {
        "focus_25" => {
            crate::focus::start(app, 25);
            Ok(())
        }
        "focus_end" => {
            crate::focus::stop(app);
            Ok(())
        }
        "pause_15" => commands::sensors_pause(app.clone(), Some(15)).map(drop),
        "pause_60" => commands::sensors_pause(app.clone(), Some(60)).map(drop),
        "pause_forever" => commands::sensors_pause(app.clone(), None).map(drop),
        "resume" => commands::set_pause(app, Pause::None).map(drop),
        "demo" => {
            suggestions::demo(app);
            Ok(())
        }
        "settings" => windows::open_settings(app).map_err(|e| e.to_string()),
        "quit" => {
            app.exit(0);
            Ok(())
        }
        other => Err(format!("unknown tray item {other}")),
    };
    if let Err(err) = result {
        log::warn!("tray action failed: {err}");
    }
}
