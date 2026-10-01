//! Undo for actions that created a file or folder (FR-ACT-04): the new item
//! goes to the Recycle Bin, so even an undo can be undone. Only paths an
//! action itself produced are ever touched, and only for 24 hours.

use std::path::Path;

use chrono::{DateTime, Utc};
use tauri::{AppHandle, Manager};

use crate::state::{AppState, lock};

/// Actions whose `Outcome::path` is something they created.
const UNDOABLE: &[&str] = &["convert", "extract_archive"];
const WINDOW_HOURS: i64 = 24;

pub fn undo_path(action: &str, produced: Option<&str>) -> Option<String> {
    UNDOABLE
        .contains(&action)
        .then(|| produced.map(str::to_owned))
        .flatten()
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
