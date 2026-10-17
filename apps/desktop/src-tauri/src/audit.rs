//! Audit log of tool calls: which tool ran, for which chat, and how it
//! ended. Never the arguments or the result, so no mail, file or page text
//! lands here.

use std::io::Write;
use std::path::Path;

use tauri::{AppHandle, Manager};

/// Past this size the log moves to audit.old.jsonl and starts again.
const MAX_BYTES: u64 = 1_000_000;

pub fn line(ts: &str, chat: &str, tool: &str, outcome: &str) -> String {
    serde_json::json!({ "ts": ts, "chat": chat, "tool": tool, "outcome": outcome }).to_string()
}

/// "error" when the tool answered with an error, else "ok".
pub fn outcome_of(result: Option<&str>) -> &'static str {
    match result {
        Some(r) if r.starts_with("Error") => "error",
        Some(_) => "ok",
        None => "none",
    }
}

fn append(dir: &Path, text: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join("audit.jsonl");
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > MAX_BYTES) {
        std::fs::rename(&path, dir.join("audit.old.jsonl"))?;
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(f, "{text}")
}

pub fn record(app: &AppHandle, chat: &str, tool: &str, outcome: &str) {
    let Ok(dir) = app.path().app_data_dir() else {
        return;
    };
    let text = line(&chrono::Utc::now().to_rfc3339(), chat, tool, outcome);
    if let Err(err) = append(&dir, &text) {
        log::debug!("audit log: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_no_content_and_rotates() {
        let l = line("t", "c1", "PROPOSE", outcome_of(Some("Error: no")));
        assert_eq!(
            l,
            r#"{"chat":"c1","outcome":"error","tool":"PROPOSE","ts":"t"}"#
        );
        let dir = std::env::temp_dir().join(format!("duskie-audit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("audit.jsonl"), vec![b'x'; MAX_BYTES as usize + 1]).unwrap();
        append(&dir, "next").unwrap();
        assert!(dir.join("audit.old.jsonl").exists());
        assert_eq!(
            std::fs::read_to_string(dir.join("audit.jsonl")).unwrap(),
            "next\n"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
