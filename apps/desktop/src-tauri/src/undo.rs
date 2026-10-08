//! Undo for actions that created a file or folder: the new item
//! goes to the Recycle Bin, so even an undo can be undone. Only paths an
//! action itself produced are ever touched, and only for 24 hours.

use std::path::Path;

use chrono::{DateTime, Utc};
use tauri::{AppHandle, Manager};

use crate::state::{AppState, lock};

/// Actions whose `Outcome::path` is something they created.
const UNDOABLE: &[&str] = &["convert", "extract_archive", "zip"];
/// A moved file's undo record: where it is now, then where it came from.
const MOVE_BACK: &str = "move-back:";
const WINDOW_HOURS: i64 = 24;

pub fn undo_path(action: &str, produced: Option<&str>) -> Option<String> {
    // A pull's record resets the repo back to where it was.
    if action == "git_pull" {
        return produced
            .filter(|p| p.starts_with(sidekick_actions::dev::RESET_TO))
            .map(str::to_owned);
    }
    if action == "git_clone" {
        return produced.map(str::to_owned);
    }
    UNDOABLE
        .contains(&action)
        .then(|| produced.map(str::to_owned))
        .flatten()
}

/// Undo for a move puts the file back where it was.
pub fn move_back(now: &str, from: &str) -> String {
    format!("{MOVE_BACK}{now}\n{from}")
}

/// Refuses anything that is not plainly a single item inside a folder.
fn safe_to_trash(path: &Path) -> bool {
    let home = dirs_home();
    path.is_absolute()
        && path.components().count() >= 3
        && path.parent().is_some()
        && home.as_deref() != Some(path)
        && home.as_deref() != path.parent()
}

fn dirs_home() -> Option<std::path::PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(Into::into)
}

pub fn undo(app: &AppHandle, id: i64) -> Result<String, String> {
    let state = app.state::<AppState>();
    let record = lock(&state.storage)
        .action(id)
        .map_err(|e| e.to_string())?
        .ok_or("That action is no longer in the history")?;
    if record.undone {
        return Err("Already undone".into());
    }
    let path = record.undo_path.ok_or("This action cannot be undone")?;
    let when = DateTime::parse_from_rfc3339(&record.ts)
        .map_err(|e| e.to_string())?
        .with_timezone(&Utc);
    if Utc::now() - when > chrono::Duration::hours(WINDOW_HOURS) {
        return Err("Undo is only kept for 24 hours".into());
    }
    if let Some(rest) = path.strip_prefix(sidekick_actions::dev::RESET_TO) {
        let (repo, sha) = rest
            .split_once('\n')
            .ok_or("This action cannot be undone")?;
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(["reset", "--keep", sha])
            .output()
            .map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(
                "Your changes since then are in the way; undo by hand with git reset".into(),
            );
        }
        lock(&state.storage)
            .mark_undone(id)
            .map_err(|e| e.to_string())?;
        return Ok("Put the repo back to before the pull".into());
    }
    if let Some(rest) = path.strip_prefix(MOVE_BACK) {
        let (now, from) = rest
            .split_once('\n')
            .ok_or("This action cannot be undone")?;
        let (now, from) = (Path::new(now), Path::new(from));
        if !now.exists() {
            return Err("It was already moved or deleted".into());
        }
        if from.exists() {
            return Err("Something else is in its old place now".into());
        }
        std::fs::rename(now, from)
            .or_else(|_| std::fs::copy(now, from).and_then(|_| std::fs::remove_file(now)))
            .map_err(|e| format!("could not move it back: {e}"))?;
        lock(&state.storage)
            .mark_undone(id)
            .map_err(|e| e.to_string())?;
        let name = from
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        return Ok(format!("Moved {name} back"));
    }
    let path = Path::new(&path);
    if !path.exists() {
        lock(&state.storage)
            .mark_undone(id)
            .map_err(|e| e.to_string())?;
        return Err("It was already moved or deleted".into());
    }
    if !safe_to_trash(path) {
        return Err("Sidekick will not remove that path".into());
    }
    trash::delete(path).map_err(|e| format!("could not move it to the Recycle Bin: {e}"))?;
    lock(&state.storage)
        .mark_undone(id)
        .map_err(|e| e.to_string())?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(format!("Moved {name} to the Recycle Bin"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_file_producing_actions_are_undoable() {
        assert_eq!(
            undo_path("convert", Some("/tmp/a.webp")),
            Some("/tmp/a.webp".into())
        );
        assert_eq!(undo_path("open_url", Some("/tmp/a.webp")), None);
        assert_eq!(undo_path("convert", None), None);
    }

    #[test]
    fn never_trashes_roots_or_home() {
        assert!(!safe_to_trash(Path::new("/")));
        assert!(!safe_to_trash(Path::new("relative/file.txt")));
        if let Some(home) = dirs_home() {
            assert!(!safe_to_trash(&home));
            assert!(!safe_to_trash(&home.join("Downloads")));
            assert!(safe_to_trash(&home.join("Downloads").join("photo.webp")));
        }
    }
}
