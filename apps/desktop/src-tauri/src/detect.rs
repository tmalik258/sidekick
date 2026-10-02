//! First-run detection: what Sidekick can set up without asking the user
//! to type anything. Code folders come from the usual places and from VS
//! Code's recent projects (WSL ones included); search folders are the
//! standard ones that exist; models are whatever Ollama already has.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    pub path: String,
    pub label: String,
    /// Git repos found in it (code folders only).
    pub repos: usize,
}

/// `file:///c%3A/Users/me/code/api` -> `C:\Users\me\code\api`, and
/// `vscode-remote://wsl%2Bubuntu-22.04/home/me/api` ->
/// `\\wsl.localhost\Ubuntu-22.04\home\me\api`.
pub fn uri_to_path(uri: &str) -> Option<PathBuf> {
    if let Some(rest) = uri.strip_prefix("file:///") {
        let decoded = percent_decode(rest);
        let windows = decoded.replace('/', "\\");
        return Some(PathBuf::from(
            if cfg!(windows) || decoded.chars().nth(1) == Some(':') {
                capitalize_drive(&windows)
            } else {
                format!("/{decoded}")
            },
        ));
    }
    let rest = uri.strip_prefix("vscode-remote://")?;
    let decoded = percent_decode(rest);
    let (authority, path) = decoded.split_once('/')?;
    let distro = authority.strip_prefix("wsl+")?;
    let distro = wsl_distro_name(distro);
    Some(PathBuf::from(format!(
        r"\\wsl.localhost\{distro}\{}",
        path.replace('/', "\\")
    )))
}

/// VS Code lowercases the distro (`ubuntu-22.04`); Windows names it
/// `Ubuntu-22.04`. UNC paths ignore case, but it reads better.
fn wsl_distro_name(d: &str) -> String {
    let mut c = d.chars();
    c.next()
        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
        .unwrap_or_default()
}

fn capitalize_drive(p: &str) -> String {
    let mut chars: Vec<char> = p.chars().collect();
    if chars.len() > 1 && chars[1] == ':' {
        chars[0] = chars[0].to_ascii_uppercase();
    }
    chars.into_iter().collect()
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(hex) = s.get(i + 1..i + 3)
            && let Ok(b) = u8::from_str_radix(hex, 16)
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Every folder URI in VS Code's `storage.json`.
pub fn recent_uris(storage_json: &str) -> Vec<String> {
    fn walk(v: &Value, out: &mut Vec<String>) {
        match v {
            Value::String(s)
                if s.starts_with("file:///") || s.starts_with("vscode-remote://wsl") =>
            {
                out.push(s.clone());
            }
            Value::Object(m) => {
                for (k, x) in m {
                    if k.starts_with("file:///") || k.starts_with("vscode-remote://wsl") {
                        out.push(k.clone());
                    }
                    walk(x, out);
                }
            }
            Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    if let Ok(v) = serde_json::from_str::<Value>(storage_json) {
        walk(&v, &mut out);
    }
    out.sort();
    out.dedup();
    out
}

fn vscode_storage_files() -> Vec<PathBuf> {
    let Some(config) = dirs::config_dir() else {
        return Vec::new();
    };
    ["Code", "Code - Insiders", "Cursor", "Windsurf"]
        .iter()
        .map(|app| {
            config
                .join(app)
                .join("User")
                .join("globalStorage")
                .join("storage.json")
        })
        .filter(|p| p.is_file())
        .collect()
}

/// Folders that hold your projects, most repos first: the usual code
/// folders plus the parents of projects recently opened in VS Code.
pub fn code_roots() -> Vec<Folder> {
    let mut counts: BTreeMap<PathBuf, usize> = BTreeMap::new();
    for root in sidekick_sensors::repos::ReposSensor::default_roots() {
        counts.entry(root).or_default();
    }
    for file in vscode_storage_files() {
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        for uri in recent_uris(&text) {
            let Some(path) = uri_to_path(&uri) else {
                continue;
            };
            if path.join(".git").exists()
                && let Some(parent) = path.parent()
                && parent.parent().is_some()
            {
                counts.entry(parent.to_path_buf()).or_default();
            }
        }
    }
    let mut out: Vec<Folder> = counts
        .into_keys()
        .map(|p| {
            let repos = sidekick_sensors::repos::find_repos(&p).len();
            Folder {
                label: label_for(&p),
                path: p.display().to_string(),
                repos,
            }
        })
        .filter(|f| f.repos > 0)
        .collect();
    out.sort_by(|a, b| b.repos.cmp(&a.repos).then_with(|| a.path.cmp(&b.path)));
    out
}

/// The folder's own name, with the WSL distro for WSL folders. Split by
/// hand so Windows paths read the same on any OS.
fn label_for(p: &Path) -> String {
    let s = p.display().to_string();
    let parts: Vec<&str> = s.split(['\\', '/']).filter(|x| !x.is_empty()).collect();
    let name = parts.last().copied().unwrap_or(&s).to_owned();
    if s.starts_with(r"\\wsl") {
        let distro = parts.get(1).copied().unwrap_or("WSL");
        return format!("{name} (WSL {distro})");
    }
    name
}

/// Standard folders worth searching that exist on this PC.
pub fn search_folders() -> Vec<Folder> {
    let mut out = Vec::new();
    let mut add = |p: Option<PathBuf>, label: &str| {
        if let Some(p) = p.filter(|p| p.is_dir())
            && !out
                .iter()
                .any(|f: &Folder| f.path == p.display().to_string())
        {
            out.push(Folder {
                path: p.display().to_string(),
                label: label.into(),
                repos: 0,
            });
        }
    };
    add(dirs::document_dir(), "Documents");
    add(dirs::desktop_dir(), "Desktop");
    add(dirs::download_dir(), "Downloads");
    add(
        dirs::home_dir().map(|h| h.join("OneDrive").join("Documents")),
        "OneDrive Documents",
    );
    add(dirs::home_dir().map(|h| h.join("Notes")), "Notes");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_vscode_folder_uris() {
        let json = r#"{
            "profileAssociations": { "workspaces": {
                "file:///c%3A/Users/me/code/api": "__default__",
                "vscode-remote://wsl%2Bubuntu-22.04/home/me/projects/web": "__default__"
            }},
            "backupWorkspaces": { "folders": [{ "folderUri": "file:///c%3A/Users/me/code/site" }] },
            "theme": "vs-dark"
        }"#;
        let uris = recent_uris(json);
        assert_eq!(uris.len(), 3);
        assert!(uris.iter().any(|u| u.ends_with("code/site")));
    }

    #[test]
    fn turns_uris_into_paths() {
        assert_eq!(
            uri_to_path("file:///c%3A/Users/me/my%20code/api").unwrap(),
            PathBuf::from(r"C:\Users\me\my code\api")
        );
        assert_eq!(
            uri_to_path("vscode-remote://wsl%2Bubuntu-22.04/home/me/web").unwrap(),
            PathBuf::from(r"\\wsl.localhost\Ubuntu-22.04\home\me\web")
        );
        assert!(uri_to_path("vscode-remote://ssh-remote%2Bbox/home/x").is_none());
        assert!(uri_to_path("https://example.com").is_none());
    }

    #[test]
    fn labels_wsl_folders() {
        assert_eq!(
            label_for(Path::new(r"\\wsl.localhost\Ubuntu-22.04\home\me\projects")),
            "projects (WSL Ubuntu-22.04)"
        );
    }
}
