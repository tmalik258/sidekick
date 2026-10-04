//! Update check: once a day, asks GitHub for the latest release and
//! offers it when it is newer. Install downloads the installer from this
//! repository's release, checks it against the release's SHA256SUMS.txt,
//! and runs it (the user clicks through it); Sidekick then quits so it can
//! be replaced. Nothing installs on its own.

use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use sidekick_core::Event;
use tauri::{AppHandle, Emitter, Manager};

use crate::state::{AppState, lock};

pub const AVAILABLE: &str = "app.update_available";
const LATEST: &str = "https://api.github.com/repos/tmalik258/sidekick/releases/latest";
const RELEASES: &str = "https://github.com/tmalik258/sidekick/releases/";
const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);
/// Tells the island an update is waiting, so it can keep a small sign up.
pub const EVENT: &str = "update://available";

/// A newer release than the one running.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Available {
    pub version: String,
    pub current: String,
    pub url: String,
}

/// The update found by the last check, kept until it is installed.
static FOUND: Mutex<Option<Available>> = Mutex::new(None);

pub fn found() -> Option<Available> {
    FOUND.lock().map(|f| f.clone()).unwrap_or(None)
}

/// Asks GitHub now. Remembers and announces a newer release; returns it,
/// or None when this is the newest.
pub async fn check(app: &AppHandle) -> Result<Option<Available>, String> {
    let current = app.package_info().version.to_string();
    let (tag, url) = latest().await?;
    let next = is_newer(&tag, &current).then(|| Available {
        version: tag.trim_start_matches('v').to_owned(),
        current,
        url,
    });
    if let Ok(mut f) = FOUND.lock() {
        *f = next.clone();
    }
    if let Some(a) = &next {
        let _ = app.emit(EVENT, a);
    }
    Ok(next)
}

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

const DOWNLOADS: &str = "https://github.com/tmalik258/sidekick/releases/download/";

/// The installer and the checksum file among a release's assets.
pub fn pick_assets(body: &serde_json::Value) -> Option<(String, String)> {
    let assets = body["assets"].as_array()?;
    let url = |pred: &dyn Fn(&str) -> bool| {
        assets
            .iter()
            .filter(|a| a["name"].as_str().is_some_and(pred))
            .filter_map(|a| a["browser_download_url"].as_str())
            .find(|u| u.starts_with(DOWNLOADS))
            .map(str::to_owned)
    };
    let installer = url(&|n| n.ends_with("-setup.exe"))?;
    let sums = url(&|n| n == "SHA256SUMS.txt")?;
    Some((installer, sums))
}

/// The expected hash of `file` in a `sha256sum`-style list.
pub fn expected_hash(sums: &str, file: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let (hash, name) = l.trim().split_once(char::is_whitespace)?;
        (name.trim().trim_start_matches('*') == file && hash.len() == 64)
            .then(|| hash.to_ascii_lowercase())
    })
}

/// Downloads, checks and runs the newest installer, then quits.
pub async fn install(app: &AppHandle) -> Result<String, String> {
    use sha2::Digest;
    let client = reqwest::Client::builder()
        .user_agent("Sidekick")
        .timeout(Duration::from_secs(300))
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
    let (installer, sums_url) = pick_assets(&body)
        .ok_or("This release has no installer with checksums; use the release page")?;
    let name = installer
        .rsplit('/')
        .next()
        .unwrap_or("Sidekick-setup.exe")
        .to_owned();
    let sums = client
        .get(&sums_url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| e.to_string())?
        .text()
        .await
        .map_err(|e| e.to_string())?;
    let want =
        expected_hash(&sums, &name).ok_or("The checksum list does not include the installer")?;
    let bytes = client
        .get(&installer)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| e.to_string())?
        .bytes()
        .await
        .map_err(|e| e.to_string())?;
    let got = format!("{:x}", sha2::Sha256::digest(&bytes));
    if got != want {
        return Err("The download did not match its checksum; nothing was installed".into());
    }
    let dir = std::env::temp_dir().join("sidekick-update");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(&name);
    std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;
    std::process::Command::new(&path)
        .spawn()
        .map_err(|e| format!("could not start the installer: {e}"))?;
    // Give the installer a moment to open, then quit so files can be replaced.
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(2)).await;
        app.exit(0);
    });
    Ok(format!("Installing {name}"))
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
        let mut offered = String::new();
        loop {
            tokio::time::sleep(Duration::from_secs(120)).await;
            let enabled = lock(&app.state::<AppState>().settings).check_updates;
            if enabled {
                match check(&app).await {
                    // The card comes once per version; the small sign stays.
                    Ok(Some(a)) if a.version != offered => {
                        offered = a.version.clone();
                        app.state::<AppState>().bus.publish(Event::new(
                            AVAILABLE,
                            "updates",
                            serde_json::json!({
                                "version": a.version,
                                "current": a.current,
                                "url": a.url,
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
    fn finds_the_installer_and_its_hash() {
        let body = serde_json::json!({ "assets": [
            { "name": "Sidekick_0.2.0_x64_en-US.msi", "browser_download_url": "https://github.com/tmalik258/sidekick/releases/download/v0.2.0/Sidekick_0.2.0_x64_en-US.msi" },
            { "name": "Sidekick_0.2.0_x64-setup.exe", "browser_download_url": "https://github.com/tmalik258/sidekick/releases/download/v0.2.0/Sidekick_0.2.0_x64-setup.exe" },
            { "name": "SHA256SUMS.txt", "browser_download_url": "https://github.com/tmalik258/sidekick/releases/download/v0.2.0/SHA256SUMS.txt" }
        ]});
        let (exe, sums) = pick_assets(&body).unwrap();
        assert!(exe.ends_with("-setup.exe"));
        assert!(sums.ends_with("SHA256SUMS.txt"));
        let elsewhere = serde_json::json!({ "assets": [
            { "name": "x-setup.exe", "browser_download_url": "https://evil.example/x-setup.exe" },
            { "name": "SHA256SUMS.txt", "browser_download_url": "https://evil.example/SHA256SUMS.txt" }
        ]});
        assert!(pick_assets(&elsewhere).is_none());
        let h = "a".repeat(64);
        let sums = format!(
            "{h}  Sidekick_0.2.0_x64-setup.exe\n{}  other.msi\n",
            "b".repeat(64)
        );
        assert_eq!(
            expected_hash(&sums, "Sidekick_0.2.0_x64-setup.exe"),
            Some(h)
        );
        assert_eq!(expected_hash(&sums, "missing.exe"), None);
    }

    #[test]
    fn compares_versions() {
        assert!(is_newer("v0.2.0", "0.1.9"));
        assert!(is_newer("1.0", "0.9.9"));
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(!is_newer("v0.1.0-beta", "0.1.0"));
        assert!(!is_newer("nightly", "0.1.0"));
    }
}
