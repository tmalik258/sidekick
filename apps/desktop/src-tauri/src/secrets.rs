//! Secrets (the Composio key) live in Windows Credential Manager, never in
//! the settings file. Elsewhere (development on Linux or macOS) they go in
//! a file only the user can read. They are stored under the app's product
//! name, so the dev build ("Sidekick Dev") never reads the installed app's.

use std::sync::OnceLock;

static SERVICE: OnceLock<String> = OnceLock::new();

/// Called once at startup, before any secret is read or written.
pub fn init(product_name: &str) {
    SERVICE
        .set(product_name.to_owned())
        .expect("secrets::init called twice");
}

fn service() -> &'static str {
    SERVICE
        .get()
        .expect("secrets::init was not called at startup")
}

#[cfg(windows)]
pub fn get(name: &str) -> Option<String> {
    keyring::Entry::new(service(), name)
        .ok()?
        .get_password()
        .ok()
        .filter(|s| !s.is_empty())
}

#[cfg(windows)]
pub fn set(name: &str, value: &str) -> Result<(), String> {
    keyring::Entry::new(service(), name)
        .and_then(|e| e.set_password(value))
        .map_err(|e| format!("could not save to Credential Manager: {e}"))
}

#[cfg(windows)]
pub fn delete(name: &str) {
    if let Ok(e) = keyring::Entry::new(service(), name) {
        let _ = e.delete_credential();
    }
}

#[cfg(not(windows))]
fn path(name: &str) -> Option<std::path::PathBuf> {
    let safe: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    Some(
        dirs::config_dir()?
            .join(service())
            .join("secrets")
            .join(safe),
    )
}

#[cfg(not(windows))]
pub fn get(name: &str) -> Option<String> {
    std::fs::read_to_string(path(name)?)
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
}

#[cfg(not(windows))]
pub fn set(name: &str, value: &str) -> Result<(), String> {
    use std::io::Write;
    let p = path(name).ok_or("no config folder")?;
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(&p).map_err(|e| e.to_string())?;
    f.write_all(value.as_bytes()).map_err(|e| e.to_string())
}

#[cfg(not(windows))]
pub fn delete(name: &str) {
    if let Some(p) = path(name) {
        let _ = std::fs::remove_file(p);
    }
}
