//! "Add for me" for Codex: a `notify` program in `~/.codex/config.toml` so
//! the island hears when a Codex turn is done, and Sidekick's MCP server
//! so Codex can search your history. Only when the user presses the button;
//! the file is backed up first and everything else in it is kept.

use std::path::{Path, PathBuf};

use crate::setup::CODEX_HOOK_URL;

/// The script Codex runs after each turn, next to Sidekick's data. Codex
/// adds the turn as JSON at the end of the command; the script passes it
/// on to Sidekick on this PC and never fails Codex.
pub const NOTIFY_SCRIPT: &str = "codex-notify.ps1";

pub fn config_path() -> Option<PathBuf> {
    let home = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".codex")))?;
    Some(home.join("config.toml"))
}

pub fn script_text() -> String {
    format!(
        "param([string]$json)\n\
         # Sent by Codex after each turn; passed on to Sidekick on this PC.\n\
         try {{\n  \
           $body = [System.Text.Encoding]::UTF8.GetBytes($json)\n  \
           Invoke-RestMethod -Uri '{CODEX_HOOK_URL}' -Method Post -ContentType 'application/json' -Body $body -TimeoutSec 3 | Out-Null\n\
         }} catch {{}}\n"
    )
}

/// A TOML literal string (no escapes needed for Windows paths).
fn lit(s: &str) -> String {
    format!("'{}'", s.replace('\'', ""))
}

fn notify_line(script: &Path) -> String {
    format!(
        "notify = [{}, {}, {}, {}, {}, {}, {}, {}, {}]",
        lit("powershell.exe"),
        lit("-NoProfile"),
        lit("-NonInteractive"),
        lit("-ExecutionPolicy"),
        lit("Bypass"),
        lit("-WindowStyle"),
        lit("Hidden"),
        lit("-File"),
        lit(&script.to_string_lossy()),
    )
}

/// Where top-level keys end: the first `[table]` line.
fn top_level_end(text: &str) -> usize {
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        if line.trim_start().starts_with('[') {
            return at;
        }
        at += line.len();
    }
    text.len()
}

/// The config with Sidekick's notify added. Another program already in
/// `notify` is left alone (Codex runs only one), and that is an error the
/// user sees.
pub fn merge_notify(text: &str, script: &Path) -> Result<String, String> {
    let end = top_level_end(text);
    let top = &text[..end];
    if let Some(line) = top
        .lines()
        .find(|l| l.trim_start().starts_with("notify") && l.contains('='))
    {
        if line.contains(NOTIFY_SCRIPT) {
            return Ok(text.to_owned());
        }
        return Err(
            "Codex already runs another program after each turn (notify in ~/.codex/config.toml). Remove it to use Sidekick's."
                .into(),
        );
    }
    let mut out = String::with_capacity(text.len() + 200);
    out.push_str("# Sidekick: tell the island when a Codex turn is done.\n");
    out.push_str(&notify_line(script));
    out.push_str("\n\n");
    out.push_str(text);
    Ok(out)
}

/// The config with `[mcp_servers.sidekick]` set to this link and token,
/// replacing an older entry.
pub fn merge_mcp(text: &str, url: &str, token: &str) -> String {
    let mut out = String::new();
    let mut skipping = false;
    for line in text.split_inclusive('\n') {
        let t = line.trim();
        if t.starts_with('[') {
            skipping = t == "[mcp_servers.sidekick]" || t.starts_with("[mcp_servers.sidekick.");
        }
        if !skipping {
            out.push_str(line);
        }
    }
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    if !out.is_empty() && !out.ends_with("\n\n") {
        out.push('\n');
    }
    out.push_str(&format!(
        "[mcp_servers.sidekick]\nurl = {}\nhttp_headers = {{ \"Authorization\" = {} }}\n",
        lit(url),
        lit(&format!("Bearer {token}")),
    ));
    out
}

pub fn has_notify(text: &str) -> bool {
    text[..top_level_end(text)].contains(NOTIFY_SCRIPT)
}

pub fn has_mcp(text: &str) -> bool {
    text.lines().any(|l| l.trim() == "[mcp_servers.sidekick]")
}

fn write_with_backup(path: &Path, old: &str, new: &str) -> Result<Option<PathBuf>, String> {
    if new == old {
        return Ok(None);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let backup = if old.is_empty() {
        None
    } else {
        let b = path.with_file_name(format!(
            "config.toml.sidekick-backup-{}",
            chrono::Local::now().format("%Y%m%d-%H%M%S")
        ));
        std::fs::write(&b, old).map_err(|e| format!("could not back up Codex's settings: {e}"))?;
        Some(b)
    };
    std::fs::write(path, new).map_err(|e| format!("could not save Codex's settings: {e}"))?;
    Ok(backup)
}

/// Writes the notify script into `data_dir` and adds it to Codex.
pub fn add_notify(data_dir: &Path) -> Result<Option<PathBuf>, String> {
    let script = data_dir.join(NOTIFY_SCRIPT);
    std::fs::create_dir_all(data_dir).map_err(|e| e.to_string())?;
    std::fs::write(&script, script_text()).map_err(|e| e.to_string())?;
    let path = config_path().ok_or("no home folder")?;
    let old = std::fs::read_to_string(&path).unwrap_or_default();
    let new = merge_notify(&old, &script)?;
    write_with_backup(&path, &old, &new)
}

pub fn add_mcp(url: &str, token: &str) -> Result<Option<PathBuf>, String> {
    let path = config_path().ok_or("no home folder")?;
    let old = std::fs::read_to_string(&path).unwrap_or_default();
    write_with_backup(&path, &old, &merge_mcp(&old, url, token))
}

/// (notify added, MCP added), from Codex's settings file.
pub fn status() -> (bool, bool) {
    let text = config_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();
    (has_notify(&text), has_mcp(&text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_notify_at_the_top_and_only_once() {
        let script = Path::new(r"C:\Users\me\AppData\Roaming\Sidekick\codex-notify.ps1");
        let old = "model = \"gpt-5\"\n\n[mcp_servers.other]\nurl = \"x\"\n";
        let new = merge_notify(old, script).unwrap();
        assert!(new.starts_with("# Sidekick"));
        assert!(new.contains(r"'C:\Users\me\AppData\Roaming\Sidekick\codex-notify.ps1'"));
        assert!(new.contains("[mcp_servers.other]"));
        assert!(has_notify(&new));
        assert_eq!(merge_notify(&new, script).unwrap(), new);
        // Someone else's notify is never replaced.
        assert!(merge_notify("notify = [\"other.exe\"]\n", script).is_err());
        // A notify key inside a table is not the top-level one.
        assert!(merge_notify("[tui]\nnotify = true\n", script).is_ok());
    }

    #[test]
    fn sets_sidekicks_server_and_replaces_an_old_one() {
        let old = "model = \"gpt-5\"\n[mcp_servers.sidekick]\nurl = 'old'\n[profiles.x]\nmodel = \"o3\"\n";
        let new = merge_mcp(old, "http://127.0.0.1:47823/mcp", "t0k");
        assert_eq!(new.matches("[mcp_servers.sidekick]").count(), 1);
        assert!(!new.contains("'old'"));
        assert!(new.contains("[profiles.x]"));
        assert!(new.contains("'Bearer t0k'"));
        assert!(has_mcp(&new));
        assert!(merge_mcp("", "http://x", "t").starts_with("[mcp_servers.sidekick]"));
    }

    #[test]
    fn script_posts_to_sidekick() {
        let s = script_text();
        assert!(s.contains(CODEX_HOOK_URL));
        assert!(s.contains("catch {}"));
    }
}
