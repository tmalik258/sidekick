//! "Add for me": puts Sidekick's hooks into `~/.claude/settings.json` and its
//! MCP server into Claude Code, only when the user presses the button. The
//! settings file is backed up first and only Sidekick's entries are added;
//! everything else in it is kept as it was.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::setup::CLAUDE_HOOK_URL;

/// Hook events Sidekick listens to, with each one's timeout in seconds.
const HOOKS: &[(&str, u32)] = &[("Stop", 5), ("Notification", 5), ("PermissionRequest", 30)];

fn points_here(groups: &Value) -> bool {
    groups.as_array().is_some_and(|g| {
        g.iter()
            .any(|group| group.to_string().contains(CLAUDE_HOOK_URL))
    })
}

/// The settings text with Sidekick's hooks added where missing. Errors when
/// the file is not a JSON object, so nothing is overwritten by mistake.
pub fn merge_hooks(text: &str) -> Result<String, String> {
    let mut v: Value = if text.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str(text)
            .map_err(|e| format!("~/.claude/settings.json is not valid JSON ({e}); fix it first"))?
    };
    let root = v
        .as_object_mut()
        .ok_or("~/.claude/settings.json is not a JSON object")?;
    let hooks = root.entry("hooks").or_insert_with(|| json!({}));
    let hooks = hooks
        .as_object_mut()
        .ok_or("\"hooks\" in ~/.claude/settings.json is not an object")?;
    for (event, timeout) in HOOKS {
        let groups = hooks.entry(*event).or_insert_with(|| json!([]));
        if points_here(groups) {
            continue;
        }
        let list = groups
            .as_array_mut()
            .ok_or(format!("hooks.{event} is not a list"))?;
        list.push(
            json!({ "hooks": [{ "type": "http", "url": CLAUDE_HOOK_URL, "timeout": timeout }] }),
        );
    }
    serde_json::to_string_pretty(&v).map_err(|e| e.to_string())
}

fn settings_path() -> Option<PathBuf> {
    Some(dirs::home_dir()?.join(".claude").join("settings.json"))
}

/// Adds the hooks, keeping a dated copy of the old file next to it.
/// Returns the backup's path, if there was a file to back up.
pub fn add_hooks() -> Result<Option<PathBuf>, String> {
    let path = settings_path().ok_or("no home folder")?;
    add_hooks_at(&path)
}

fn add_hooks_at(path: &Path) -> Result<Option<PathBuf>, String> {
    let old = std::fs::read_to_string(path).unwrap_or_default();
    let new = merge_hooks(&old)?;
    if new.trim() == old.trim() {
        return Ok(None);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let backup = if old.is_empty() {
        None
    } else {
        let b = path.with_file_name(format!(
            "settings.json.sidekick-backup-{}",
            chrono::Local::now().format("%Y%m%d-%H%M%S")
        ));
        std::fs::write(&b, &old).map_err(|e| format!("could not back up the settings: {e}"))?;
        Some(b)
    };
    // Write next to it, then swap, so a crash never leaves half a file.
    let tmp = path.with_extension("json.sidekick-tmp");
    std::fs::write(&tmp, new).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())?;
    Ok(backup)
}

/// Adds Sidekick's MCP server to Claude Code with its own command, so
/// Sidekick never edits `~/.claude.json`.
pub async fn add_mcp(claude: &Path, url: &str, token: &str) -> Result<(), String> {
    let mut cmd = tokio::process::Command::new(claude);
    cmd.args([
        "mcp",
        "add",
        "--scope",
        "user",
        "--transport",
        "http",
        "sidekick",
        url,
        "--header",
        &format!("Authorization: Bearer {token}"),
    ])
    .stdin(std::process::Stdio::null())
    .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let out = tokio::time::timeout(std::time::Duration::from_secs(30), cmd.output())
        .await
        .map_err(|_| "Claude Code did not answer".to_string())?
        .map_err(|e| format!("could not run Claude Code: {e}"))?;
    let text =
        String::from_utf8_lossy(&out.stderr).to_string() + &String::from_utf8_lossy(&out.stdout);
    if out.status.success() || text.contains("already exists") {
        Ok(())
    } else {
        Err(format!("claude mcp add failed: {}", text.trim()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_only_missing_hooks_and_keeps_the_rest() {
        let old = r#"{
          "model": "opus",
          "hooks": {
            "Stop": [{ "hooks": [{ "type": "command", "command": "notify-send done" }] }],
            "Notification": [{ "hooks": [{ "type": "http", "url": "http://127.0.0.1:47821/claude-code" }] }]
          }
        }"#;
        let new: Value = serde_json::from_str(&merge_hooks(old).unwrap()).unwrap();
        assert_eq!(new["model"], "opus");
        let stop = new["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 2, "the user's own Stop hook stays");
        assert_eq!(stop[0]["hooks"][0]["command"], "notify-send done");
        assert_eq!(
            new["hooks"]["Notification"].as_array().unwrap().len(),
            1,
            "not added twice"
        );
        assert_eq!(
            new["hooks"]["PermissionRequest"][0]["hooks"][0]["timeout"],
            30
        );
        assert_eq!(
            merge_hooks(&merge_hooks(old).unwrap()).unwrap(),
            merge_hooks(old).unwrap()
        );
    }

    #[test]
    fn refuses_to_touch_a_broken_file() {
        assert!(merge_hooks("{ not json").is_err());
        assert!(merge_hooks("[1, 2]").is_err());
        assert!(merge_hooks(r#"{"hooks": 3}"#).is_err());
        assert!(merge_hooks("").unwrap().contains("PermissionRequest"));
    }

    #[test]
    fn backs_up_before_writing() {
        let dir = std::env::temp_dir().join(format!("sidekick-claude-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        std::fs::write(&path, r#"{"model":"opus"}"#).unwrap();
        let backup = add_hooks_at(&path).unwrap().unwrap();
        assert_eq!(
            std::fs::read_to_string(&backup).unwrap(),
            r#"{"model":"opus"}"#
        );
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains(CLAUDE_HOOK_URL)
        );
        assert!(
            add_hooks_at(&path).unwrap().is_none(),
            "nothing to do the second time"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
