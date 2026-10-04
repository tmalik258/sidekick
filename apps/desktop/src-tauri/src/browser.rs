//! The browser side of Sidekick: the pairing code the extension needs, and
//! the actions that send it commands.

use std::path::Path;

use serde_json::{Value, json};
use sidekick_actions::Outcome;
use tauri::{AppHandle, Manager};

use crate::state::AppState;

/// Reads a pairing code, creating a random one on first run. It lives in
/// the app's data folder, readable only by this user.
pub fn load_or_create_token(dir: &Path) -> String {
    load_or_create_secret(dir, "browser-token")
}

pub fn load_or_create_secret(dir: &Path, name: &str) -> String {
    let path = dir.join(name);
    if let Ok(t) = std::fs::read_to_string(&path) {
        let t = t.trim().to_owned();
        if t.len() >= 32 {
            return t;
        }
    }
    let mut bytes = [0u8; 24];
    if getrandom::fill(&mut bytes).is_err() {
        log::warn!("no secure randomness; the browser bridge stays locked");
        return String::new();
    }
    let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let _ = std::fs::create_dir_all(dir);
    if let Err(err) = std::fs::write(&path, &token) {
        log::warn!("could not save the browser pairing code: {err}");
    }
    token
}

pub async fn run(app: &AppHandle, action: &str, _args: &Value) -> Result<Outcome, String> {
    let bridge = app.state::<AppState>().browser.clone();
    let msg = |m: &str| Outcome {
        message: m.to_owned(),
        path: None,
    };
    match action {
        "browser_close_duplicates" => {
            bridge.send(json!({ "type": "close_duplicates" }));
            Ok(msg("Closing duplicate tabs"))
        }
        "browser_save_session" => {
            bridge.send(json!({ "type": "save_session" }));
            Ok(msg("Saved your tabs to bookmarks"))
        }
        other => Err(format!("unknown browser action {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairing_code_is_created_once_and_reused() {
        let dir = std::env::temp_dir().join(format!("sidekick-token-{}", std::process::id()));
        let a = load_or_create_token(&dir);
        let b = load_or_create_token(&dir);
        assert_eq!(a.len(), 48);
        assert_eq!(a, b);
        let _ = std::fs::remove_dir_all(dir);
    }
}
