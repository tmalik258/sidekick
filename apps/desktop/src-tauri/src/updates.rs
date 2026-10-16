//! Update check: once a day, asks GitHub for the latest release and
//! offers it when it is newer. Install downloads the installer from this
//! repository's release, checks it against the release's SHA256SUMS.txt,
//! and runs it (the user clicks through it); Duskie then quits so it can
//! be replaced. Nothing installs on its own.

use std::sync::Mutex;
use std::time::Duration;

use duskie_core::Event;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::state::{AppState, lock};

pub const AVAILABLE: &str = "app.update_available";
const LATEST: &str = "https://api.github.com/repos/tmalik258/sidekick/releases/latest";
const RELEASES: &str = "https://github.com/tmalik258/sidekick/releases/";
const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);
/// Tells the island an update is waiting, so it can keep a small sign up.
pub const EVENT: &str = "update://available";
pub const PROGRESS_EVENT: &str = "update://progress";

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Preparing,
    Downloading,
    Verifying,
    Opening,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub phase: Phase,
    pub done: u64,
    pub total: Option<u64>,
    pub error: Option<String>,
}

impl Progress {
    fn active(&self) -> bool {
        self.phase != Phase::Failed
    }
}

static PROGRESS: Mutex<Option<Progress>> = Mutex::new(None);

pub fn progress() -> Option<Progress> {
    PROGRESS.lock().ok().and_then(|p| p.clone())
}

fn report(app: &AppHandle, activity_id: &str, phase: Phase, done: u64, total: Option<u64>) {
    let progress = Progress {
        phase,
        done,
        total,
        error: None,
    };
    crate::activity::report(
        app,
        activity_id,
        "update",
        "Updating Duskie",
        match phase {
            Phase::Preparing => crate::activity::Phase::Running,
            Phase::Downloading => crate::activity::Phase::Downloading,
            Phase::Verifying => crate::activity::Phase::Verifying,
            Phase::Opening => crate::activity::Phase::AwaitingUser,
            Phase::Failed => crate::activity::Phase::Failed,
        },
        Some(done),
        total,
        None,
    );
    *lock(&PROGRESS) = Some(progress.clone());
    let _ = app.emit(PROGRESS_EVENT, progress);
}

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
    let mut announce = false;
    if let Ok(mut f) = FOUND.lock() {
        announce = next.as_ref().is_some_and(|a| {
            f.as_ref()
                .is_none_or(|previous| previous.version != a.version)
        });
        *f = next.clone();
    }
    if let Some(a) = &next {
        let _ = app.emit(EVENT, a);
        if announce {
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
    // A rebuilt release can retain installers from an older version.
    // Match the release tag rather than taking the first executable.
    let version = body["tag_name"].as_str()?.trim_start_matches('v');
    // Installers were named Sidekick_ before the rename to Duskie.
    let prefixes = [format!("Duskie_{version}_"), format!("Sidekick_{version}_")];
    let installer = url(&|n| {
        prefixes.iter().any(|p| n.starts_with(p.as_str())) && n.ends_with("_x64-setup.exe")
    })?;
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
    {
        let mut progress = lock(&PROGRESS);
        if progress.as_ref().is_some_and(Progress::active) {
            return Ok("An update is already in progress".into());
        }
        *progress = Some(Progress {
            phase: Phase::Preparing,
            done: 0,
            total: None,
            error: None,
        });
    }
    let activity_id = format!("update:{}", ulid::Ulid::new());
    crate::activity::report(
        app,
        &activity_id,
        "update",
        "Updating Duskie",
        crate::activity::Phase::Running,
        Some(0),
        None,
        None,
    );
    let _ = app.emit(PROGRESS_EVENT, progress());
    let result = install_inner(app, &activity_id).await;
    if let Err(error) = &result {
        let failed = {
            let mut state = lock(&PROGRESS);
            let previous = state.as_ref();
            let failed = Progress {
                phase: Phase::Failed,
                done: previous.map_or(0, |p| p.done),
                total: previous.and_then(|p| p.total),
                error: Some(error.clone()),
            };
            *state = Some(failed.clone());
            failed
        };
        crate::activity::report(
            app,
            &activity_id,
            "update",
            "Updating Duskie",
            crate::activity::Phase::Failed,
            Some(failed.done),
            failed.total,
            Some(error.clone()),
        );
        let _ = app.emit(PROGRESS_EVENT, failed);
    }
    result
}

async fn install_inner(app: &AppHandle, activity_id: &str) -> Result<String, String> {
    use sha2::Digest;
    let client = reqwest::Client::builder()
        .user_agent("Duskie")
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(30 * 60))
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
        .unwrap_or("Duskie-setup.exe")
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
    report(app, activity_id, Phase::Downloading, 0, None);
    let response = client
        .get(&installer)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| e.to_string())?;
    let total = response.content_length();
    let bytes = download_bytes(response, |done, total| {
        report(app, activity_id, Phase::Downloading, done, total);
    })
    .await?;
    report(
        app,
        activity_id,
        Phase::Verifying,
        bytes.len() as u64,
        total,
    );
    let got = format!("{:x}", sha2::Sha256::digest(&bytes));
    if got != want {
        return Err("The download did not match its checksum; nothing was installed".into());
    }
    let dir = std::env::temp_dir().join("sidekick-update");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(&name);
    std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;
    report(app, activity_id, Phase::Opening, bytes.len() as u64, total);
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

async fn download_bytes(
    mut response: reqwest::Response,
    mut notify: impl FnMut(u64, Option<u64>),
) -> Result<Vec<u8>, String> {
    let total = response.content_length();
    let mut bytes = Vec::new();
    notify(0, total);
    let mut last = std::time::Instant::now();
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        bytes.extend_from_slice(&chunk);
        if last.elapsed() >= Duration::from_millis(200) {
            notify(bytes.len() as u64, total);
            last = std::time::Instant::now();
        }
    }
    notify(bytes.len() as u64, total);
    Ok(bytes)
}

async fn latest() -> Result<(String, String), String> {
    let client = reqwest::Client::builder()
        .user_agent("Duskie")
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
        tokio::time::sleep(Duration::from_secs(120)).await;
        loop {
            let enabled = lock(&app.state::<AppState>().settings).check_updates;
            if enabled && let Err(err) = check(&app).await {
                log::debug!("update check: {err}");
            }
            tokio::time::sleep(CHECK_EVERY).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn streams_download_progress_before_completion() {
        use std::io::{Read, Write};
        let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = server.local_addr().unwrap();
        let thread = std::thread::spawn(move || {
            let (mut socket, _) = server.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = [0; 2048];
            let _ = socket.read(&mut request).unwrap();
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\nabc")
                .unwrap();
            std::thread::sleep(Duration::from_millis(250));
            socket.write_all(b"de").unwrap();
            std::thread::sleep(Duration::from_millis(250));
            socket.write_all(b"f").unwrap();
        });
        let response = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap()
            .get(format!("http://{address}"))
            .send()
            .await
            .unwrap();
        let mut updates = Vec::new();
        let bytes = download_bytes(response, |done, total| updates.push((done, total)))
            .await
            .unwrap();
        thread.join().unwrap();
        assert_eq!(bytes, b"abcdef");
        assert_eq!(updates.first(), Some(&(0, Some(6))));
        assert_eq!(updates.last(), Some(&(6, Some(6))));
        assert!(updates.iter().any(|(done, _)| *done > 0 && *done < 6));
    }

    #[tokio::test]
    async fn incomplete_download_returns_an_error() {
        use std::io::{Read, Write};
        let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = server.local_addr().unwrap();
        let thread = std::thread::spawn(move || {
            let (mut socket, _) = server.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = [0; 2048];
            let _ = socket.read(&mut request).unwrap();
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\nabc")
                .unwrap();
        });
        let response = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap()
            .get(format!("http://{address}"))
            .send()
            .await
            .unwrap();
        assert!(download_bytes(response, |_, _| {}).await.is_err());
        thread.join().unwrap();
    }

    #[test]
    fn finds_the_installer_and_its_hash() {
        let body = serde_json::json!({ "tag_name": "v0.2.0", "assets": [
            { "name": "Sidekick_0.1.0_x64-setup.exe", "browser_download_url": "https://github.com/tmalik258/sidekick/releases/download/v0.2.0/Sidekick_0.1.0_x64-setup.exe" },
            { "name": "Sidekick_0.2.0_x64_en-US.msi", "browser_download_url": "https://github.com/tmalik258/sidekick/releases/download/v0.2.0/Sidekick_0.2.0_x64_en-US.msi" },
            { "name": "Sidekick_0.2.0_x64-setup.exe", "browser_download_url": "https://github.com/tmalik258/sidekick/releases/download/v0.2.0/Sidekick_0.2.0_x64-setup.exe" },
            { "name": "SHA256SUMS.txt", "browser_download_url": "https://github.com/tmalik258/sidekick/releases/download/v0.2.0/SHA256SUMS.txt" }
        ]});
        let (exe, sums) = pick_assets(&body).unwrap();
        assert!(exe.ends_with("Sidekick_0.2.0_x64-setup.exe"));
        assert!(sums.ends_with("SHA256SUMS.txt"));
        let elsewhere = serde_json::json!({ "tag_name": "v0.2.0", "assets": [
            { "name": "x-setup.exe", "browser_download_url": "https://evil.example/x-setup.exe" },
            { "name": "SHA256SUMS.txt", "browser_download_url": "https://evil.example/SHA256SUMS.txt" }
        ]});
        assert!(pick_assets(&elsewhere).is_none());
        let mut missing = body.clone();
        missing["tag_name"] = serde_json::json!("v0.3.0");
        assert!(pick_assets(&missing).is_none());
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
