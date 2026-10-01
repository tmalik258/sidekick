//! App and site deny-list (FR-SET-02, FR-RAG-08): events from these apps
//! and sites are dropped before they are stored, indexed or seen by skills.
//! Password managers are on the list from the start.

use serde_json::Value;
use sidekick_core::Event;
use tauri::{AppHandle, Manager};

use crate::state::{AppState, lock};

fn host(url: &str) -> Option<String> {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let host = rest.split(['/', '?', '#']).next()?.split('@').next_back()?;
    let host = host.split(':').next()?.to_ascii_lowercase();
    (!host.is_empty()).then_some(host)
}

/// `example.com` also covers `www.example.com` and `app.example.com`.
pub fn site_denied(url: &str, sites: &[String]) -> bool {
    let Some(h) = host(url) else { return false };
    sites.iter().any(|s| {
        let s = s.trim().trim_start_matches("*.").to_ascii_lowercase();
        !s.is_empty() && (h == s || h.ends_with(&format!(".{s}")))
    })
}

pub fn app_denied(exe: &str, apps: &[String]) -> bool {
    let exe = exe.trim().to_ascii_lowercase();
    !exe.is_empty()
        && apps.iter().any(|a| {
            let a = a.trim().to_ascii_lowercase();
            a == exe || a.trim_end_matches(".exe") == exe.trim_end_matches(".exe")
        })
}

/// True when the event must be dropped. `last_exe` is the app in front.
pub fn denied(event: &Event, last_exe: &str, apps: &[String], sites: &[String]) -> bool {
    let p = &event.payload;
    let url = |k: &str| p[k].as_str().unwrap_or_default();
    match event.kind.as_str() {
        "window.focused" => app_denied(p["exe"].as_str().unwrap_or_default(), apps),
        // A copy belongs to the app it was made in.
        "clipboard.changed" => app_denied(last_exe, apps) || site_denied(url("text").trim(), sites),
        k if k.starts_with("browser.") => site_denied(url("url"), sites),
        _ => false,
    }
}

pub fn check(app: &AppHandle, event: &Event) -> bool {
    let state = app.state::<AppState>();
    let last_exe = lock(&state.last_window)
        .as_ref()
        .and_then(|w: &Value| w["exe"].as_str().map(str::to_owned))
        .unwrap_or_default();
    let s = lock(&state.settings);
    denied(event, &last_exe, &s.deny_apps, &s.deny_sites)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(kind: &str, payload: Value) -> Event {
        Event::new(kind, "test", payload)
    }

    #[test]
    fn drops_denied_apps_and_sites() {
        let apps = vec!["KeePassXC.exe".to_owned(), "slack".to_owned()];
        let sites = vec!["bank.com".to_owned()];
        assert!(denied(
            &ev(
                "window.focused",
                serde_json::json!({ "exe": "keepassxc.exe" })
            ),
            "",
            &apps,
            &sites
        ));
        assert!(denied(
            &ev("window.focused", serde_json::json!({ "exe": "slack.exe" })),
            "",
            &apps,
            &sites
        ));
        assert!(!denied(
            &ev("window.focused", serde_json::json!({ "exe": "code.exe" })),
            "",
            &apps,
            &sites
        ));
        let copy = ev("clipboard.changed", serde_json::json!({ "text": "hello" }));
        assert!(denied(&copy, "keepassxc.exe", &apps, &sites));
        assert!(!denied(&copy, "code.exe", &apps, &sites));
        let page = |u: &str| ev("browser.long_read", serde_json::json!({ "url": u }));
        assert!(denied(
            &page("https://online.bank.com/accounts"),
            "",
            &apps,
            &sites
        ));
        assert!(denied(
            &page("https://user@bank.com:443/x"),
            "",
            &apps,
            &sites
        ));
        assert!(!denied(&page("https://notbank.com/"), "", &apps, &sites));
        assert!(!denied(
            &ev("file.download_completed", serde_json::json!({})),
            "keepassxc.exe",
            &apps,
            &sites
        ));
    }
}
