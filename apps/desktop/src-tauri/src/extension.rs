//! Installing the browser extension. Browsers do not let apps add
//! extensions on their own, so Sidekick does everything around the one
//! step the browser keeps for the user: it puts the extension in a fixed
//! folder, copies that folder's path, and opens the browser's extensions
//! page. Once loaded, the extension asks to connect by itself.

use std::path::{Path, PathBuf};

use serde::Serialize;
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Guide {
    /// What was copied: the folder (Chromium) or manifest.json (Firefox).
    pub copied: String,
    pub steps: Vec<String>,
}

pub fn steps(firefox: bool) -> Vec<String> {
    if firefox {
        vec![
            "Click Load Temporary Add-on.".into(),
            "Paste the path into the file name box (it is copied) and press Enter.".into(),
            "Press Allow on Sidekick's island when it asks.".into(),
            "Firefox forgets temporary add-ons when it restarts; do this again then, or use a signed build.".into(),
        ]
    } else {
        vec![
            "Turn on Developer mode (top right).".into(),
            "Click Load unpacked.".into(),
            "Paste the path into the folder box (it is copied) and press Enter, then Select Folder.".into(),
            "Press Allow on Sidekick's island when it asks.".into(),
        ]
    }
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
    arboard::Clipboard::new()
        .and_then(|mut c| c.set_text(copied.clone()))
        .map_err(|e| format!("could not copy the path: {e}"))?;
    let exe = executor(&app.state::<AppState>())
        .capabilities()
        .browser(browser)
        .map(|b| b.path.clone())
        .ok_or("That browser is not installed")?;
    std::process::Command::new(exe)
        .arg(page)
        .spawn()
        .map_err(|e| format!("could not open the browser: {e}"))?;
    Ok(Guide {
        copied,
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
}
