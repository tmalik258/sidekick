//! Login lookup in the user's password manager (FR-BRW-01), through its own
//! CLI: 1Password (`op`, unlocked by the desktop app) or Bitwarden (`bw`,
//! unlocked with `bw unlock` and `BW_SESSION`). Sidekick never stores,
//! logs, or sends a credential anywhere except to the browser tab that asked.

use std::path::Path;
use std::time::Duration;

use serde_json::Value;
use tokio::process::Command;

use crate::ActionError;

const TIMEOUT: Duration = Duration::from_secs(20);

/// A login for one site. Deliberately not `Debug`, so it cannot end up in a
/// log line by accident.
pub struct Login {
    pub username: String,
    pub password: String,
}

/// True when `url`'s host is `domain` or a subdomain of it, or the other way
/// round (a vault entry for `github.com` matches `gist.github.com`).
pub fn url_matches(url: &str, domain: &str) -> bool {
    let host = url
        .split("://")
        .nth(1)
        .unwrap_or(url)
        .split(['/', '?', '#', ':'])
        .next()
        .unwrap_or_default()
        .trim_start_matches("www.")
        .to_ascii_lowercase();
    let domain = domain.trim_start_matches("www.").to_ascii_lowercase();
    !host.is_empty()
        && !domain.is_empty()
        && (host == domain
            || host.ends_with(&format!(".{domain}"))
            || domain.ends_with(&format!(".{host}")))
}

async fn json(cmd: &mut Command) -> Result<Value, ActionError> {
    cmd.kill_on_drop(true).stdin(std::process::Stdio::null());
    #[cfg(windows)]
    {
        cmd.creation_flags(0x0800_0000);
    }
    let out = tokio::time::timeout(TIMEOUT, cmd.output())
        .await
        .map_err(|_| ActionError::Failed("the password manager took too long".into()))?
        .map_err(|e| ActionError::Failed(e.to_string()))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let line = err
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("failed");
        return Err(ActionError::Failed(line.trim().chars().take(160).collect()));
    }
    serde_json::from_slice(&out.stdout)
        .map_err(|_| ActionError::Failed("unexpected answer from the password manager".into()))
}

/// 1Password: list logins, pick the first whose URL matches, read its
/// username and password.
pub async fn onepassword(op: &Path, domain: &str) -> Result<Login, ActionError> {
    let items =
        json(Command::new(op).args(["item", "list", "--categories", "Login", "--format", "json"]))
            .await?;
    let id = pick_op_item(&items, domain)
        .ok_or_else(|| ActionError::Failed(format!("no 1Password login for {domain}")))?;
    let fields = json(Command::new(op).args([
        "item",
        "get",
        &id,
        "--fields",
        "label=username,label=password",
        "--reveal",
        "--format",
        "json",
    ]))
    .await?;
    op_login(&fields).ok_or_else(|| ActionError::Failed("that login has no password".into()))
}

fn pick_op_item(items: &Value, domain: &str) -> Option<String> {
    items.as_array()?.iter().find_map(|item| {
        let urls = item["urls"].as_array()?;
        urls.iter()
            .filter_map(|u| u["href"].as_str())
            .any(|href| url_matches(href, domain))
            .then(|| item["id"].as_str().map(str::to_owned))
            .flatten()
    })
}

fn op_login(fields: &Value) -> Option<Login> {
    let value = |label: &str| {
        fields
            .as_array()?
            .iter()
            .find(|f| f["label"] == label || f["id"] == label)
            .and_then(|f| f["value"].as_str())
            .map(str::to_owned)
    };
    Some(Login {
        username: value("username").unwrap_or_default(),
        password: value("password")?,
    })
}

/// Bitwarden: needs an unlocked session (`BW_SESSION`) in Sidekick's
/// environment. Picks the first login whose URI matches.
pub async fn bitwarden(bw: &Path, domain: &str) -> Result<Login, ActionError> {
    if std::env::var("BW_SESSION").map_or(true, |s| s.trim().is_empty()) {
        return Err(ActionError::Failed(
            "Bitwarden is locked: run bw unlock and set BW_SESSION, then restart Sidekick".into(),
        ));
    }
    let items = json(Command::new(bw).args(["list", "items", "--search", domain])).await?;
    bw_login(&items, domain)
        .ok_or_else(|| ActionError::Failed(format!("no Bitwarden login for {domain}")))
}

fn bw_login(items: &Value, domain: &str) -> Option<Login> {
    items.as_array()?.iter().find_map(|item| {
        let login = &item["login"];
        let matches = login["uris"]
            .as_array()?
            .iter()
            .filter_map(|u| u["uri"].as_str())
            .any(|uri| url_matches(uri, domain));
        if !matches {
            return None;
        }
        Some(Login {
            username: login["username"].as_str().unwrap_or_default().to_owned(),
            password: login["password"].as_str()?.to_owned(),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn matches_hosts_and_subdomains_only() {
        assert!(url_matches("https://github.com/login", "github.com"));
        assert!(url_matches("https://www.github.com", "github.com"));
        assert!(url_matches("https://github.com", "gist.github.com"));
        assert!(url_matches("github.com", "github.com"));
        assert!(!url_matches("https://github.com.evil.io", "github.com"));
        assert!(!url_matches("https://notgithub.com", "github.com"));
        assert!(!url_matches("", "github.com"));
    }

    #[test]
    fn picks_1password_items_by_url() {
        let items = json!([
            {"id": "a", "urls": [{"href": "https://gitlab.com"}]},
            {"id": "b", "urls": [{"href": "https://github.com/login"}]},
        ]);
        assert_eq!(pick_op_item(&items, "github.com").as_deref(), Some("b"));
        assert_eq!(pick_op_item(&items, "example.com"), None);
        let fields = json!([
            {"id": "username", "label": "username", "value": "me"},
            {"id": "password", "label": "password", "value": "s3cret"},
        ]);
        let login = op_login(&fields).unwrap();
        assert_eq!(
            (login.username.as_str(), login.password.as_str()),
            ("me", "s3cret")
        );
    }

    #[test]
    fn picks_bitwarden_logins_by_uri() {
        let items = json!([
            {"login": {"username": "x", "password": "1", "uris": [{"uri": "https://github.com.evil.io"}]}},
            {"login": {"username": "me", "password": "2", "uris": [{"uri": "https://github.com"}]}},
        ]);
        let login = bw_login(&items, "github.com").unwrap();
        assert_eq!(login.password, "2");
        assert!(bw_login(&items, "gitlab.com").is_none());
    }
}
