//! Installing the browser extension. Browsers do not let apps add
//! extensions on their own, so Sidekick does everything around the one
//! step the browser keeps for the user: it puts the extension in a fixed
//! folder, copies that folder's path, and opens the browser's extensions
//! page. Once loaded, the extension asks to connect by itself.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Manager};

use crate::state::{AppState, executor};

/// The extension shipped with the app: next to the executable once
/// installed, or the repo's copy while developing.
fn shipped(app: &AppHandle) -> Option<PathBuf> {
    let bundled = app.path().resource_dir().ok().map(|d| d.join("extension"));
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../extension");
    [bundled, Some(repo)]
        .into_iter()
        .flatten()
        .find(|p| p.join("manifest.json").is_file())
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// A folder that stays put across Sidekick updates, so the browser keeps
/// finding the extension. Refreshed when Sidekick ships a newer one.
pub fn folder(app: &AppHandle) -> Result<PathBuf, String> {
    let from = shipped(app).ok_or("The extension is missing from this install")?;
    let to = dirs::data_local_dir()
        .ok_or("no app data folder")?
        .join("Sidekick")
        .join("extension");
    let same = std::fs::read(from.join("manifest.json")).ok()
        == std::fs::read(to.join("manifest.json")).ok()
        && std::fs::read(from.join("background.js")).ok()
            == std::fs::read(to.join("background.js")).ok();
    if !same {
        let _ = std::fs::remove_dir_all(&to);
        copy_dir(&from, &to).map_err(|e| format!("could not copy the extension: {e}"))?;
    }
    Ok(to)
}

/// The extensions page of each browser, and whether it is Firefox-like.
fn extensions_page(browser: &str) -> Option<(&'static str, bool)> {
    Some(match browser {
        "chrome" => ("chrome://extensions/", false),
        "edge" => ("edge://extensions/", false),
        "brave" => ("brave://extensions/", false),
        "firefox" | "zen" => ("about:debugging#/runtime/this-firefox", true),
        _ => return None,
    })
}

/// Chromium user-data root for the last-used profile (Windows paths).
fn chromium_user_data(browser: &str) -> Option<PathBuf> {
    let local = dirs::data_local_dir()?;
    Some(match browser {
        "chrome" => local.join("Google").join("Chrome").join("User Data"),
        "edge" => local.join("Microsoft").join("Edge").join("User Data"),
        "brave" => local
            .join("BraveSoftware")
            .join("Brave-Browser")
            .join("User Data"),
        _ => return None,
    })
}

/// Reads Chromium's `Local State` so we open the last profile and skip the
/// profile picker. Without `--profile-directory`, Chrome shows the picker and
/// then drops the `chrome://extensions/` URL (Zen/Firefox do not).
fn last_used_profile(user_data: &Path) -> String {
    parse_last_used_profile(
        &std::fs::read_to_string(user_data.join("Local State")).unwrap_or_default(),
    )
}

fn parse_last_used_profile(local_state: &str) -> String {
    serde_json::from_str::<Value>(local_state)
        .ok()
        .and_then(|v| {
            v.get("profile")?
                .get("last_used")?
                .as_str()
                .map(str::to_owned)
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Default".into())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Guide {
    /// The folder (Chromium) or manifest.json (Firefox) to load.
    pub copied: String,
    /// The extensions page. Browsers refuse to open it for other apps, so
    /// the user pastes it into the address bar.
    pub page: String,
    pub steps: Vec<String>,
}

pub fn steps(firefox: bool) -> Vec<String> {
    let open = "If the extensions page is not showing, paste its address into the address bar and press Enter (it is copied).";
    if firefox {
        vec![
            open.into(),
            "Click Load Temporary Add-on.".into(),
            "Click Copy file path below, paste it into the file box and press Enter.".into(),
            "Press Allow on Sidekick's island when it asks.".into(),
            "Firefox forgets temporary add-ons when it restarts; do this again then, or use a signed build.".into(),
        ]
    } else {
        vec![
            open.into(),
            "Turn on Developer mode (top right), then click Load unpacked.".into(),
            "Copy folder path below (or its Alt key), paste it into the folder box and press Enter, then Select Folder.".into(),
            "Press Allow on Sidekick's island when it asks.".into(),
        ]
    }
}

fn spawn_browser(exe: &Path, args: &[String]) -> Result<(), String> {
    let mut cmd = Command::new(exe);
    cmd.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: do not flash a console behind the browser.
        cmd.creation_flags(0x0800_0000);
    }
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("could not open the browser: {e}"))
}

/// Opens `browser` on its extensions page with the path on the clipboard.
pub async fn install(app: &AppHandle, browser: &str) -> Result<Guide, String> {
    let (page, firefox) = extensions_page(browser).ok_or("Unknown browser")?;
    let dir = folder(app)?;
    let copied = if firefox {
        dir.join("manifest.json")
    } else {
        dir.clone()
    }
    .display()
    .to_string();
    // The address goes first: the browser will not open its own extensions
    // page when asked by another app, so it is pasted into the address bar.
    arboard::Clipboard::new()
        .and_then(|mut c| c.set_text(page.to_owned()))
        .map_err(|e| format!("could not copy the address: {e}"))?;
    let exe = executor(&app.state::<AppState>())
        .capabilities()
        .browser(browser)
        .map(|b| b.path.clone())
        .ok_or("That browser is not installed")?;

    let mut args = Vec::new();
    if !firefox {
        // Target the last Chromium profile so the extensions URL is not lost
        // after the multi-profile picker (cold start).
        if let Some(data) = chromium_user_data(browser) {
            let profile = last_used_profile(&data);
            args.push(format!("--profile-directory={profile}"));
        }
        args.push("--new-window".into());
    }
    args.push(page.to_owned());
    spawn_browser(Path::new(&exe), &args)?;

    Ok(Guide {
        copied,
        page: page.to_owned(),
        steps: steps(firefox),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knows_each_browsers_extensions_page() {
        assert_eq!(extensions_page("edge"), Some(("edge://extensions/", false)));
        assert!(extensions_page("zen").unwrap().1);
        assert!(extensions_page("safari").is_none());
        assert_eq!(steps(false).len(), 4);
    }

    #[test]
    fn reads_last_used_chromium_profile() {
        assert_eq!(
            parse_last_used_profile(r#"{"profile":{"last_used":"Profile 2"}}"#),
            "Profile 2"
        );
        assert_eq!(parse_last_used_profile("{}"), "Default");
        assert_eq!(parse_last_used_profile(""), "Default");
    }
}
