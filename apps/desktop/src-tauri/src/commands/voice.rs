//! Voice: models, listening, speaking and the welcome lines.

use super::*;

#[tauri::command]
pub fn voice_status(app: AppHandle) -> crate::voice::VoiceStatus {
    crate::voice::status(&app)
}

#[tauri::command]
pub fn voice_download(app: AppHandle) -> CmdResult<()> {
    crate::voice::download(&app)
}

#[tauri::command]
pub fn voice_cancel_download(app: AppHandle) {
    crate::voice::cancel_download(&app);
}

#[tauri::command]
pub fn voice_listen(app: AppHandle) -> CmdResult<()> {
    crate::voice::listen(&app)
}

#[tauri::command]
pub fn voice_stop(app: AppHandle) {
    crate::voice::stop(&app);
}

/// The welcome line and when each part of it is heard.
#[tauri::command]
pub fn voice_welcome(app: AppHandle) -> crate::voice::WelcomeSpeech {
    crate::voice::welcome_speech(&app)
}

/// Speaks welcome step `step` (Next and Back in the welcome).
#[tauri::command]
pub fn voice_welcome_step(app: AppHandle, step: u32) {
    if !lock(&app.state::<AppState>().settings).onboarded {
        crate::voice::speak_welcome_step(&app, step);
    }
}

/// Says a short line, such as "Done. Composio is connected."
#[tauri::command]
pub fn voice_say(app: AppHandle, text: String) {
    let text: String = text.chars().take(200).collect();
    crate::voice::say_now(&app, &text);
}

/// Off the UI thread: it waits while a new voice loads.
#[tauri::command]
pub async fn voice_test(app: AppHandle) -> CmdResult<()> {
    tauri::async_runtime::spawn_blocking(move || crate::voice::test(&app))
        .await
        .map_err(|e| e.to_string())?
}
