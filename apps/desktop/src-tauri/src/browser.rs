//! The browser side of Sidekick: the pairing code the extension needs, and
//! the actions that send it commands (FR-BRW-01, FR-BRW-04).

use std::path::Path;

use serde_json::{Value, json};
use sidekick_actions::{Outcome, passwords};
use tauri::{AppHandle, Manager};

use crate::state::{AppState, executor};

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

pub async fn run(app: &AppHandle, action: &str, args: &Value) -> Result<Outcome, String> {
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
        "browser_fill" => {
            let domain = args["domain"].as_str().unwrap_or_default().to_owned();
            if domain.is_empty() {
                return Err("no site to fill".into());
            }
            let caps = executor(&app.state::<AppState>()).capabilities().clone();
            let login = match args["manager"].as_str() {
                Some("op") => {
                    let op = caps.op.as_ref().ok_or("1Password CLI is not installed")?;
                    passwords::onepassword(op, &domain).await
                }
                _ => {
                    let bw = caps.bw.as_ref().ok_or("Bitwarden CLI is not installed")?;
                    passwords::bitwarden(bw, &domain).await
                }
            }
            .map_err(|e| e.to_string())?;
            // Straight to the extension; the credential is never logged or stored.
            bridge.send(json!({
                "type": "fill",
                "domain": domain,
                "tab": args["tab"].as_str().and_then(|t| t.parse::<u64>().ok()),
                "username": login.username,
                "password": login.password,
            }));
            Ok(msg(&format!("Filled your {domain} login")))
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
