//! Downloads housekeeping. Both actions only touch files
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

/// Installers older than this were most likely run already.
pub const INSTALLER_AFTER: Duration = Duration::from_secs(2 * 24 * 60 * 60);
/// Files newer than this are left where they are; you may still want them.
const SORT_AFTER: Duration = Duration::from_secs(24 * 60 * 60);

fn ext_of(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default()
}

pub fn is_installer(path: &Path) -> bool {
    matches!(ext_of(path).as_str(), "exe" | "msi" | "msix" | "appx")
}

/// The folder a loose download is sorted into, or None to leave it.
pub fn sort_folder(path: &Path) -> Option<&'static str> {
    Some(match ext_of(path).as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "avif" | "bmp" | "heic" | "svg" => "Images",
        "pdf" | "doc" | "docx" | "odt" | "rtf" | "txt" | "md" | "ppt" | "pptx" | "xls" | "xlsx"
        | "csv" => "Documents",
        "zip" | "rar" | "7z" | "tar" | "gz" | "tgz" => "Archives",
        "exe" | "msi" | "msix" | "appx" => "Installers",
        "mp4" | "mov" | "mkv" | "webm" | "avi" => "Videos",
        "mp3" | "wav" | "flac" | "m4a" | "ogg" => "Audio",
        _ => return None,
    })
}

/// Files directly in `dir` that are older than `cutoff` and can be sorted.
pub fn loose_files(dir: &Path, cutoff: SystemTime) -> Vec<PathBuf> {
    old_files(dir, cutoff)
        .into_iter()
        .map(|(p, _)| p)
        .filter(|p| sort_folder(p).is_some())
        .collect()
}

/// Installers in `dir` older than `cutoff`.
pub fn old_installers(dir: &Path, cutoff: SystemTime) -> Vec<(PathBuf, u64)> {
    old_files(dir, cutoff)
        .into_iter()
        .filter(|(p, _)| is_installer(p))
        .collect()
}

/// Moves loose files older than a day into folders by kind, inside
/// Downloads. A name that is taken gets a number, nothing is overwritten.
pub fn sort_in(dir: &Path) -> Result<Outcome, ActionError> {
    let files = loose_files(dir, SystemTime::now() - SORT_AFTER);
    if files.is_empty() {
        return Ok(Outcome::msg("Nothing to sort"));
    }
    let mut moved = 0;
    for path in &files {
        let Some(folder) = sort_folder(path) else {
            continue;
        };
        let to = dir.join(folder);
        if std::fs::create_dir_all(&to).is_ok() && crate::files::move_into(path, &to).is_ok() {
            moved += 1;
        }
    }
    let mut out = Outcome::msg(format!("Sorted {moved} files into folders"));
    out.path = Some(dir.display().to_string());
    Ok(out)
}

pub fn sort_downloads() -> Result<Outcome, ActionError> {
    sort_in(&downloads()?)
}

/// Sends installers older than two days to the Recycle Bin.
pub fn clean_installers() -> Result<Outcome, ActionError> {
    let dir = downloads()?;
    let files = old_installers(&dir, SystemTime::now() - INSTALLER_AFTER);
    if files.is_empty() {
        return Ok(Outcome::msg("No old installers"));
    }
    let paths: Vec<&PathBuf> = files.iter().map(|(p, _)| p).collect();
    trash::delete_all(paths).map_err(fail)?;
    let mb = files.iter().map(|(_, s)| s).sum::<u64>() / 1_000_000;
    Ok(Outcome::msg(format!(
        "Moved {} installers ({mb} MB) to the Recycle Bin",
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

    #[test]
    fn sorts_by_kind_without_overwriting() {
        let dir = std::env::temp_dir().join(format!("sidekick-sort-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("Images")).unwrap();
        std::fs::write(dir.join("a.png"), b"new").unwrap();
        std::fs::write(dir.join("Images").join("a.png"), b"old").unwrap();
        std::fs::write(dir.join("setup.exe"), b"x").unwrap();
        std::fs::write(dir.join("notes.xyz"), b"x").unwrap();
        let future = SystemTime::now() + Duration::from_secs(60);
        assert_eq!(loose_files(&dir, future).len(), 2);
        assert_eq!(old_installers(&dir, future).len(), 1);
        assert_eq!(sort_folder(Path::new("x.unknown")), None);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
