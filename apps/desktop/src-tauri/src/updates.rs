//! Update check (P5): once a day, asks GitHub for the latest release and
//! offers it when it is newer. Nothing is downloaded or installed on its
//! own; the button opens the release page.

use std::time::Duration;

use sidekick_core::Event;
use tauri::{AppHandle, Manager};

use crate::state::{AppState, lock};

pub const AVAILABLE: &str = "app.update_available";
const LATEST: &str = "https://api.github.com/repos/tmalik258/sidekick/releases/latest";
const RELEASES: &str = "https://github.com/tmalik258/sidekick/releases/";
const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

/// "v1.2.3" or "1.2.3" as numbers; pre-release suffixes are ignored.
fn parse(v: &str) -> Option<(u64, u64, u64)> {
    let v = v.trim().trim_start_matches('v');
    let core = v.split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
    Some((
        parts.next()??,
        parts.next().flatten().unwrap_or(0),
        parts.next().flatten().unwrap_or(0),
    ))
}

pub fn is_newer(latest: &str, current: &str) -> bool {
    matches!((parse(latest), parse(current)), (Some(l), Some(c)) if l > c)
}

async fn latest() -> Result<(String, String), String> {
    let client = reqwest::Client::builder()
        .user_agent("Sidekick")
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;
    let body: serde_json::Value = client
        .get(LATEST)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let tag = body["tag_name"].as_str().ok_or("no tag")?.to_owned();
    // Only ever link to this repository's release pages.
    let url = body["html_url"]
        .as_str()
        .filter(|u| u.starts_with(RELEASES))
        .unwrap_or(RELEASES)
        .to_owned();
    Ok((tag, url))
}

pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let current = app.package_info().version.to_string();
        let mut offered = String::new();
        loop {
            tokio::time::sleep(Duration::from_secs(120)).await;
            let enabled = lock(&app.state::<AppState>().settings).check_updates;
            if enabled {
                match latest().await {
                    Ok((tag, url)) if is_newer(&tag, &current) && tag != offered => {
                        offered = tag.clone();
                        app.state::<AppState>().bus.publish(Event::new(
                            AVAILABLE,
                            "updates",
                            serde_json::json!({
                                "version": tag.trim_start_matches('v'),
                                "current": current,
                                "url": url,
                            }),
                        ));
                    }
                    Ok(_) => {}
                    Err(err) => log::debug!("update check: {err}"),
                }
            }
            tokio::time::sleep(CHECK_EVERY).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions() {
        assert!(is_newer("v0.2.0", "0.1.9"));
        assert!(is_newer("1.0", "0.9.9"));
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(!is_newer("v0.1.0-beta", "0.1.0"));
        assert!(!is_newer("nightly", "0.1.0"));
    }
}
