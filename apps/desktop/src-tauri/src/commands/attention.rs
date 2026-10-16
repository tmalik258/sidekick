//! What Duskie has learned about which notifications matter, and the
//! profile that decides how loud they are. Every change goes through
//! `apply_settings`, so Settings and the island stay in step.

use chrono::Utc;
use duskie_core::attention::Profile;
use serde::Serialize;

use super::*;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LessonRow {
    pub key: String,
    /// Points added to or taken from the score; negative means less.
    pub shift: i32,
    /// Seconds since the epoch.
    pub updated: i64,
}

fn edit(app: &AppHandle, f: impl FnOnce(&mut duskie_core::NotificationSettings)) -> CmdResult<()> {
    let mut next = lock(&app.state::<AppState>().settings).clone();
    f(&mut next.notifications);
    apply_settings(app, next).map(|_| ())
}

/// Every sender and source Duskie has learned something about.
#[tauri::command]
pub fn attention_lessons(app: AppHandle) -> Vec<LessonRow> {
    let now = Utc::now().timestamp();
    lock(&app.state::<AppState>().settings)
        .notifications
        .lessons
        .list(now)
        .into_iter()
        .map(|(key, shift, updated)| LessonRow {
            key,
            shift,
            updated,
        })
        .collect()
}

/// You acted on something from `key`: it counts for more next time.
#[tauri::command]
pub fn attention_acted(app: AppHandle, key: String) -> CmdResult<()> {
    let now = Utc::now().timestamp();
    edit(&app, |n| n.lessons.acted(&key, now))
}

/// Show less: counts toward scoring. Dismiss has no command on purpose.
#[tauri::command]
pub fn attention_show_less(app: AppHandle, key: String) -> CmdResult<()> {
    let now = Utc::now().timestamp();
    edit(&app, |n| n.lessons.show_less(&key, now))
}

/// Forgets one key, a group (`group:mail:`), or everything (`all`).
#[tauri::command]
pub fn attention_reset(app: AppHandle, scope: String) -> CmdResult<()> {
    edit(&app, |n| {
        if scope == "all" {
            n.lessons.reset_all();
        } else if let Some(prefix) = scope.strip_prefix("group:") {
            n.lessons.reset_group(prefix);
        } else {
            n.lessons.reset(&scope);
        }
    })
}

/// "normal" or "calm".
#[tauri::command]
pub fn attention_set_profile(app: AppHandle, profile: String) -> CmdResult<()> {
    let profile = match profile.as_str() {
        "normal" => Profile::Normal,
        "calm" => Profile::Calm,
        _ => return Err("pick normal or calm".into()),
    };
    edit(&app, |n| n.profile = profile)
}

/// Where the Gmail connection stands.
#[tauri::command]
pub fn mail_status() -> crate::mail::Status {
    crate::mail::status()
}

/// Opens Google's sign-in and connects the account.
#[tauri::command]
pub async fn mail_connect(app: AppHandle) -> CmdResult<crate::mail::Status> {
    crate::mail::connect(&app).await?;
    Ok(crate::mail::status())
}

/// Signs out and deletes the saved sign-in.
#[tauri::command]
pub async fn mail_disconnect(app: AppHandle) -> crate::mail::Status {
    crate::mail::disconnect_account(&app).await;
    crate::mail::status()
}

/// Connects a mail account over IMAP (Advanced). The password is checked,
/// then kept in the keychain.
#[tauri::command]
pub async fn mail_connect_imap(
    app: AppHandle,
    host: String,
    port: u16,
    user: String,
    password: String,
) -> CmdResult<crate::mail::Status> {
    let (host, user) = (host.trim().to_owned(), user.trim().to_owned());
    if host.is_empty() || user.is_empty() || password.is_empty() {
        return Err("Fill in the server, user and password.".into());
    }
    let account = crate::imap::ImapAccount { host, port, user };
    crate::mail::connect_imap(&app, account, password).await?;
    Ok(crate::mail::status())
}
