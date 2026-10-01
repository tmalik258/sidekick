//! Downloads housekeeping (FR-FILE-05, 07). Both actions only touch files
//! directly inside the Downloads folder and send them to the Recycle Bin,
//! so they can be restored. Neither ever runs at Auto.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::{ActionError, Outcome, fail};

pub const OLD_AFTER: Duration = Duration::from_secs(30 * 24 * 60 * 60);

fn downloads() -> Result<PathBuf, ActionError> {
    dirs::download_dir().ok_or_else(|| ActionError::Failed("no Downloads folder".into()))
}

fn inside(dir: &Path, path: &Path) -> bool {
    path.parent()
        .and_then(|p| p.canonicalize().ok())
        .zip(dir.canonicalize().ok())
        .is_some_and(|(p, d)| p == d)
}

/// Sends one downloaded file to the Recycle Bin.
pub fn trash_download(path: &Path) -> Result<Outcome, ActionError> {
    let dir = downloads()?;
    if !path.is_file() || !inside(&dir, path) {
        return Err(ActionError::Invalid(
            "only files in Downloads can be removed here".into(),
        ));
    }
    trash::delete(path).map_err(fail)?;
    Ok(Outcome::msg(format!(
        "Moved {} to the Recycle Bin",
        path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    )))
}

/// Files in `dir` (not folders) last changed before `cutoff`.
pub fn old_files(dir: &Path, cutoff: SystemTime) -> Vec<(PathBuf, u64)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            let name = e.file_name().to_string_lossy().into_owned();
            (meta.is_file()
                && !name.starts_with('.')
                && name != "desktop.ini"
                && meta.modified().ok()? < cutoff)
                .then(|| (e.path(), meta.len()))
        })
        .collect()
}

/// Sends Downloads files older than 30 days to the Recycle Bin.
pub fn clean_downloads() -> Result<Outcome, ActionError> {
    let dir = downloads()?;
    let cutoff = SystemTime::now() - OLD_AFTER;
    let files = old_files(&dir, cutoff);
    if files.is_empty() {
        return Ok(Outcome::msg("Nothing older than 30 days"));
    }
    let paths: Vec<&PathBuf> = files.iter().map(|(p, _)| p).collect();
    trash::delete_all(paths).map_err(fail)?;
    let mb = files.iter().map(|(_, s)| s).sum::<u64>() / 1_000_000;
    Ok(Outcome::msg(format!(
        "Moved {} old files ({mb} MB) to the Recycle Bin",
        files.len()
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_only_old_files() {
        let dir = std::env::temp_dir().join(format!("sidekick-clean-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("folder")).unwrap();
        std::fs::write(dir.join("new.zip"), b"x").unwrap();
        std::fs::write(dir.join("desktop.ini"), b"x").unwrap();
        assert!(old_files(&dir, SystemTime::now() - OLD_AFTER).is_empty());
        let future = SystemTime::now() + Duration::from_secs(60);
        let names: Vec<String> = old_files(&dir, future)
            .into_iter()
            .map(|(p, _)| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["new.zip"], "folders and desktop.ini stay");
        assert!(!inside(&dir.join("folder"), &dir.join("new.zip")));
        assert!(inside(&dir, &dir.join("new.zip")));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
