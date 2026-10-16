//! Brings an existing Sidekick install over to Duskie on first launch: its
//! data folders, saved secrets, autostart entry, and Duskie's entries in
//! Claude Code and Codex. Each step is safe to run again; a step that fails
//! leaves the old data where it was and is retried on the next launch.

use std::path::{Path, PathBuf};

use duskie_core::migrate::{self, Outcome};
use tauri::{AppHandle, Manager};

/// The app's identifier and product name before the rename, worked out from
/// the current ones so the dev build maps to the old dev build.
pub fn old_names(identifier: &str, product: &str) -> (String, String) {
    (
        identifier.replacen("app.duskie.", "app.sidekick.", 1),
        product.replacen("Duskie", "Sidekick", 1),
    )
}

pub const OLD_DB: &str = "sidekick.db";
pub const DB: &str = "duskie.db";

/// Where the data lived and lives now, when something was moved.
pub struct Moved {
    pub old_data: PathBuf,
    pub new_data: PathBuf,
}

static MOVED: std::sync::OnceLock<Moved> = std::sync::OnceLock::new();

/// Moves the old app folders into the new ones. Runs before Tauri starts,
/// so no window, log or setting has touched the new folders yet. Uses the
/// same folders Tauri's path resolver does.
pub fn move_data(identifier: &str, product: &str) {
    let (old_id, old_product) = old_names(identifier, product);
    let mut bases: Vec<PathBuf> = [
        ::dirs::config_dir(),
        ::dirs::data_dir(),
        ::dirs::data_local_dir(),
        ::dirs::cache_dir(),
    ]
    .into_iter()
    .flatten()
    .collect();
    bases.dedup();
    for base in &bases {
        let (old, new) = (base.join(&old_id), base.join(identifier));
        if only_extension_link(&old) {
            continue;
        }
        let moved_here = move_one(&old, &new).is_some();
        if moved_here {
            keep_extension_path(&old, &new);
        }
        if moved_here && ::dirs::data_dir().is_some_and(|d| d == *base) {
            let _ = MOVED.set(Moved {
                old_data: old,
                new_data: new,
            });
        }
    }
    // Secrets saved in a file (development on Linux or macOS).
    if !cfg!(windows)
        && let Some(base) = ::dirs::config_dir()
    {
        move_one(&base.join(&old_product), &base.join(product));
    }
    // The work log in Documents, only when that folder holds nothing else
    // (a folder the user named Sidekick for their own files stays put).
    if let Some(docs) = ::dirs::document_dir() {
        let old = docs.join("Sidekick");
        let only_log = std::fs::read_dir(&old)
            .map(|it| it.flatten().map(|e| e.file_name()).collect::<Vec<_>>())
            .is_ok_and(|names| names == ["worklog.md"]);
        if only_log {
            move_one(&old, &docs.join("Duskie"));
        }
    }
    if let Some(data) = ::dirs::data_dir().map(|d| d.join(identifier)) {
        match migrate::rename_db(&data, OLD_DB, DB) {
            Ok(true) => log::info!("renamed the database to {DB}"),
            Ok(false) => {}
            Err(err) => log::warn!("could not rename the database: {err}"),
        }
    }
}

/// Browsers load the unpacked extension from the old folder. A junction
/// there pointing at the moved folder keeps it working without asking the
/// user to load it again.
#[cfg(windows)]
fn keep_extension_path(old: &Path, new: &Path) {
    use std::os::windows::process::CommandExt;
    let (link, target) = (old.join("extension"), new.join("extension"));
    if !target.is_dir() || link.exists() || std::fs::create_dir_all(old).is_err() {
        return;
    }
    let made = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&link)
        .arg(&target)
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .output()
        .is_ok_and(|o| o.status.success());
    if !made {
        log::warn!("could not keep the browser extension's old folder working");
    }
}

#[cfg(not(windows))]
fn keep_extension_path(_old: &Path, _new: &Path) {}

/// The old folder holds nothing but that junction: already moved.
fn only_extension_link(old: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(old) else {
        return false;
    };
    let entries: Vec<_> = entries.flatten().collect();
    entries.len() == 1
        && entries[0].file_name() == "extension"
        && entries[0]
            .path()
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink())
}

/// What `move_data` moved this launch, if anything.
pub fn moved() -> Option<&'static Moved> {
    MOVED.get()
}

/// Moves `old` into `new`. Returns `old` when anything was moved.
fn move_one(old: &Path, new: &Path) -> Option<PathBuf> {
    match migrate::migrate_dir(old, new, &["logs"]) {
        Ok(Outcome::NothingToMigrate) => None,
        Ok(Outcome::Moved(n)) => {
            log::info!("moved {n} files from {}", old.display());
            Some(old.to_owned())
        }
        Ok(Outcome::MovedWithLeftovers(n)) => {
            log::warn!(
                "moved {n} files from {}; it still has files Duskie already had, so it was kept",
                old.display()
            );
            Some(old.to_owned())
        }
        Err(err) => {
            log::warn!(
                "could not move {}: {err}; trying again next launch",
                old.display()
            );
            None
        }
    }
}

/// Copies saved secrets from the old service name, checks each copy, and
/// only then deletes the old entry.
#[cfg(windows)]
pub fn move_secrets(app: &AppHandle) {
    let config = app.config();
    let product = config.product_name.as_deref().unwrap_or("Duskie");
    let (_, old_product) = old_names(&config.identifier, product);
    for name in credential_names(&old_product) {
        let Ok(old) = keyring::Entry::new(&old_product, &name) else {
            continue;
        };
        let Ok(value) = old.get_password() else {
            continue;
        };
        if crate::secrets::get(&name).is_none()
            && let Err(err) = crate::secrets::set(&name, &value)
        {
            log::warn!("could not move a saved secret: {err}");
            continue;
        }
        if crate::secrets::get(&name).is_some() {
            let _ = old.delete_credential();
        }
    }
}

#[cfg(not(windows))]
pub fn move_secrets(_app: &AppHandle) {}

/// Names of the Credential Manager entries saved under `service`. The
/// keyring crate stores them as `{name}.{service}`.
#[cfg(windows)]
fn credential_names(service: &str) -> Vec<String> {
    use windows_sys::Win32::Security::Credentials::{CREDENTIALW, CredEnumerateW, CredFree};
    let filter: Vec<u16> = format!("*.{service}")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let mut count = 0u32;
    let mut creds: *mut *mut CREDENTIALW = std::ptr::null_mut();
    // SAFETY: the filter is NUL-terminated; on success Windows hands back
    // `count` pointers that stay valid until CredFree.
    if unsafe { CredEnumerateW(filter.as_ptr(), 0, &mut count, &mut creds) } == 0 {
        return Vec::new();
    }
    let suffix = format!(".{service}");
    let mut names = Vec::new();
    for i in 0..count as usize {
        // SAFETY: i < count, as above.
        let target = unsafe { wide((**creds.add(i)).TargetName) };
        if let Some(name) = target.strip_suffix(&suffix) {
            names.push(name.to_owned());
        }
    }
    // SAFETY: allocated by CredEnumerateW and freed once.
    unsafe { CredFree(creds.cast()) };
    names
}

#[cfg(windows)]
unsafe fn wide(p: *const u16) -> String {
    if p.is_null() {
        return String::new();
    }
    let mut len = 0;
    // SAFETY: the caller passes a NUL-terminated string.
    unsafe {
        while *p.add(len) != 0 {
            len += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(p, len))
    }
}

/// Moves "start with Windows" from the old app's entry to this one.
#[cfg(windows)]
pub fn move_autostart(app: &AppHandle) {
    use std::os::windows::process::CommandExt;
    use tauri_plugin_autostart::ManagerExt;
    const RUN: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    let config = app.config();
    let product = config.product_name.as_deref().unwrap_or("Duskie");
    let (_, old_product) = old_names(&config.identifier, product);
    let reg = |args: &[&str]| {
        std::process::Command::new("reg")
            .args(args)
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .output()
            .is_ok_and(|o| o.status.success())
    };
    if !reg(&["query", RUN, "/v", &old_product]) {
        return;
    }
    if let Err(err) = app.autolaunch().enable() {
        log::warn!("could not keep starting with Windows: {err}");
        return;
    }
    reg(&["delete", RUN, "/v", &old_product, "/f"]);
}

#[cfg(not(windows))]
pub fn move_autostart(_app: &AppHandle) {}

/// After the data moved: Codex's notify script path and both tools' MCP
/// server entries still name the old app. Rewrites only Duskie's entries,
/// with a backup of each file first.
pub fn fix_integrations(app: &AppHandle, moved: &Moved, token: &str) {
    let url = format!("http://127.0.0.1:{}/mcp", crate::mcp::PORT);
    if let Err(err) = crate::codex_config::after_move(&moved.old_data, &moved.new_data, &url, token)
    {
        log::warn!("could not update Codex's settings: {err}");
    }
    if !claude_has_old_server() {
        return;
    }
    let path = {
        let state = app.state::<crate::state::AppState>();
        crate::state::lock(&state.settings)
            .ai
            .claude_code
            .path
            .trim()
            .to_owned()
    };
    let claude = if path.is_empty() {
        match which::which("claude") {
            Ok(p) => p,
            Err(_) => return,
        }
    } else {
        PathBuf::from(path)
    };
    let token = token.to_owned();
    tauri::async_runtime::spawn(async move {
        let result = async {
            crate::claude_config::add_mcp(&claude, &url, &token).await?;
            crate::claude_config::remove_mcp(&claude, "sidekick").await
        }
        .await;
        if let Err(err) = result {
            log::warn!("could not rename Duskie's server in Claude Code: {err}");
        }
    });
}

/// Claude Code still lists the server under its old name. Only reads the
/// file; Claude Code's own command makes the change.
fn claude_has_old_server() -> bool {
    let Some(home) = ::dirs::home_dir() else {
        return false;
    };
    std::fs::read_to_string(home.join(".claude.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .is_some_and(|v| v["mcpServers"]["sidekick"].is_object())
}

#[cfg(test)]
mod tests {
    #[test]
    fn maps_release_and_dev_names() {
        assert_eq!(
            super::old_names("app.duskie.desktop", "Duskie"),
            ("app.sidekick.desktop".into(), "Sidekick".into())
        );
        assert_eq!(
            super::old_names("app.duskie.desktop.dev", "Duskie Dev"),
            ("app.sidekick.desktop.dev".into(), "Sidekick Dev".into())
        );
    }
}
