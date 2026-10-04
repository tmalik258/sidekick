//! The notification inbox, recipes and what desktop actions learned.

use super::*;

/// The notification inbox: whether reading works, recent items, apps seen.
#[tauri::command]
pub fn notifications_status(app: AppHandle) -> crate::inbox::Status {
    crate::inbox::status(&app)
}

/// One app's level from Settings (auto, now, soon, digest or never).
#[tauri::command]
pub fn notifications_set_level(app: AppHandle, from: String, level: String) -> CmdResult<String> {
    if level == "auto" {
        return crate::inbox::clear_level(&app, &from);
    }
    let level =
        crate::inbox::Level::parse(&level).ok_or("pick auto, now, soon, digest or never")?;
    crate::inbox::set_level(&app, &from, level)
}

/// Whether Windows' Do Not Disturb is on right now (None: cannot tell).
#[tauri::command]
pub async fn dnd_get() -> Option<bool> {
    tauri::async_runtime::spawn_blocking(sidekick_actions::dnd::state)
        .await
        .ok()
        .flatten()
}

/// Switches Windows' Do Not Disturb, quietly: pop-ups stop while every
/// notification still reaches Sidekick.
#[tauri::command]
pub async fn dnd_set(on: bool) -> CmdResult<String> {
    tauri::async_runtime::spawn_blocking(move || sidekick_actions::pc::set_dnd(on))
        .await
        .map_err(|e| e.to_string())?
        .map(|o| o.message)
        .map_err(|e| e.to_string())
}

/// Saves a recipe from Settings or Ask's Save as recipe.
#[tauri::command]
pub fn recipe_save(app: AppHandle, recipe: sidekick_core::Recipe) -> CmdResult<String> {
    crate::recipes::save(&app, recipe)
}

#[tauri::command]
pub fn recipe_delete(app: AppHandle, id: String) -> CmdResult<String> {
    crate::recipes::delete(&app, &id)
}

#[tauri::command]
pub fn recipe_run(app: AppHandle, id: String) -> CmdResult<String> {
    crate::recipes::run_by_id(&app, &id)
}

/// Forgets which controls worked where (Settings > Privacy).
#[tauri::command]
pub fn know_how_clear(app: AppHandle) {
    crate::act::forget_know_how(&app);
}

/// How many buttons the guide card shows; they get Alt+1..N (0 drops them).
#[tauri::command]
pub fn guide_keys(app: AppHandle, buttons: usize) {
    crate::suggestions::set_guide_keys(&app, buttons);
}
