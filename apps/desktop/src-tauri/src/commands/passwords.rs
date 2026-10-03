//! Saving passwords to the browsers and mirroring them.

use super::*;

#[tauri::command]
pub fn password_save_status(
    app: AppHandle,
    id: String,
    displayed: bool,
) -> CmdResult<crate::password_save::PasswordPrompt> {
    if displayed
        && !suggestions::current(&app)
            .is_some_and(|s| s.id == id && s.skill_id == crate::password_save::SKILL_ID)
    {
        return Err("that password prompt is not displayed".into());
    }
    crate::password_save::status(&app, &id, displayed)
}

#[tauri::command]
pub fn password_save_draft(
    app: AppHandle,
    id: String,
) -> CmdResult<crate::password_save::PasswordEditDraft> {
    crate::password_save::draft(&app, &id)
}

#[tauri::command]
pub async fn password_save_commit(
    app: AppHandle,
    id: String,
    username: Option<String>,
    password: Option<String>,
    override_existing: bool,
    source: Option<String>,
) -> CmdResult<String> {
    crate::password_save::commit(
        &app,
        &id,
        username,
        password,
        override_existing,
        source,
        false,
    )
    .await
}

#[tauri::command]
pub fn password_save_cancel(app: AppHandle, id: String) -> CmdResult<()> {
    crate::password_save::cancel(&app, &id)
}

#[tauri::command]
pub async fn passwords_mirror(app: AppHandle) -> CmdResult<String> {
    crate::password_save::mirror(&app).await
}

#[tauri::command]
pub fn passwords_mirror_status(app: AppHandle) -> crate::password_save::MirrorStatus {
    crate::password_save::mirror_status(&app)
}

#[tauri::command]
pub fn passwords_mirror_cancel(app: AppHandle) {
    crate::password_save::cancel_mirror(&app);
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PasswordBrowserInfo {
    id: String,
    name: String,
    /// Selected local save / fill target (default when selection is null).
    enabled: bool,
}

#[tauri::command]
pub fn password_browsers(app: AppHandle) -> Vec<PasswordBrowserInfo> {
    let state = app.state::<AppState>();
    let settings = lock(&state.settings);
    let caps = executor(&state).capabilities().clone();
    let configured = &settings.password_browsers;
    caps.browsers
        .iter()
        .filter(|b| sidekick_actions::passwords::user_data_dir(&b.id).is_some())
        .map(|b| PasswordBrowserInfo {
            id: b.id.clone(),
            name: b.label().to_owned(),
            enabled: configured.as_ref().is_none_or(|ids| ids.contains(&b.id)),
        })
        .collect()
}
