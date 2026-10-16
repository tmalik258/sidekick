//! Moves a user's saved data to a new folder when the app's identifier
//! changes. Entries are renamed into place (same parent, so no copying and
//! no extra disk space); anything the new folder already has is kept. A run
//! that stops halfway simply continues on the next launch, and running it
//! again once finished does nothing.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// No old folder, so there is nothing to bring over.
    NothingToMigrate,
    /// Moved this many files, and the old folder is gone.
    Moved(usize),
    /// Moved this many files; the old folder still holds files the new one
    /// already had, so it was left in place for the user to check.
    MovedWithLeftovers(usize),
}

/// The old app folder next to `new`: same parent, named `old_id`. None when
/// `new` has no parent.
pub fn legacy_sibling(new: &Path, old_id: &str) -> Option<PathBuf> {
    Some(new.parent()?.join(old_id))
}

/// Moves everything in `old` into `new`, keeping whatever `new` already has.
/// The old folder is removed once nothing is left in it except files under
/// `disposable` subfolders (such as old logs).
pub fn migrate_dir(old: &Path, new: &Path, disposable: &[&str]) -> io::Result<Outcome> {
    if !old.is_dir() {
        return Ok(Outcome::NothingToMigrate);
    }
    fs::create_dir_all(new)?;
    let moved = move_into(old, new)?;
    let left = leftovers(old, disposable)?;
    if left.is_empty() {
        fs::remove_dir_all(old)?;
        Ok(Outcome::Moved(moved))
    } else {
        Ok(Outcome::MovedWithLeftovers(moved))
    }
}

fn move_into(from: &Path, to: &Path) -> io::Result<usize> {
    let mut moved = 0;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        let kind = entry.file_type()?;
        if !target.exists() {
            fs::rename(entry.path(), &target)?;
            moved += if kind.is_dir() {
                count_files(&target)?
            } else {
                1
            };
        } else if kind.is_dir() && target.is_dir() {
            moved += move_into(&entry.path(), &target)?;
        }
    }
    Ok(moved)
}

fn count_files(dir: &Path) -> io::Result<usize> {
    let mut n = 0;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            n += count_files(&entry.path())?;
        } else {
            n += 1;
        }
    }
    Ok(n)
}

/// Files still in `dir` that are not inside a `disposable` top-level folder.
fn leftovers(dir: &Path, disposable: &[&str]) -> io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        if disposable.iter().any(|d| name == *d) {
            continue;
        }
        if entry.file_type()?.is_dir() {
            collect_files(&entry.path(), &mut out)?;
        } else {
            out.push(entry.path());
        }
    }
    Ok(out)
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            collect_files(&entry.path(), out)?;
        } else {
            out.push(entry.path());
        }
    }
    Ok(())
}

/// Renames `old` to `new` inside `dir` when `new` does not exist yet, along
/// with SQLite's `-wal` and `-shm` side files.
pub fn rename_db(dir: &Path, old: &str, new: &str) -> io::Result<bool> {
    let (from, to) = (dir.join(old), dir.join(new));
    if !from.exists() || to.exists() {
        return Ok(false);
    }
    for suffix in ["-wal", "-shm"] {
        let side = dir.join(format!("{old}{suffix}"));
        if side.exists() {
            fs::rename(side, dir.join(format!("{new}{suffix}")))?;
        }
    }
    fs::rename(from, to)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("migrate-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn seed(old: &Path) {
        fs::create_dir_all(old.join("skills/a")).unwrap();
        fs::create_dir_all(old.join("voice-models")).unwrap();
        fs::write(old.join("settings.json"), "{\"muted\":true}").unwrap();
        fs::write(old.join("sidekick.db"), "db").unwrap();
        fs::write(old.join("skills/a/SKILL.md"), "skill").unwrap();
        fs::write(old.join("voice-models/asr.onnx"), "model").unwrap();
    }

    #[test]
    fn moves_everything_and_removes_the_old_folder() {
        let root = tmp("move");
        let (old, new) = (root.join("old"), root.join("new"));
        seed(&old);
        assert_eq!(migrate_dir(&old, &new, &[]).unwrap(), Outcome::Moved(4));
        assert_eq!(
            fs::read_to_string(new.join("settings.json")).unwrap(),
            "{\"muted\":true}"
        );
        assert!(new.join("skills/a/SKILL.md").exists());
        assert!(new.join("voice-models/asr.onnx").exists());
        assert!(!old.exists());
        assert_eq!(
            migrate_dir(&old, &new, &[]).unwrap(),
            Outcome::NothingToMigrate
        );
    }

    #[test]
    fn nothing_to_do_without_an_old_folder() {
        let root = tmp("none");
        assert_eq!(
            migrate_dir(&root.join("old"), &root.join("new"), &[]).unwrap(),
            Outcome::NothingToMigrate
        );
    }

    #[test]
    fn keeps_what_the_new_folder_has() {
        let root = tmp("keep");
        let (old, new) = (root.join("old"), root.join("new"));
        seed(&old);
        fs::create_dir_all(&new).unwrap();
        fs::write(new.join("settings.json"), "new").unwrap();
        assert_eq!(
            migrate_dir(&old, &new, &[]).unwrap(),
            Outcome::MovedWithLeftovers(3)
        );
        assert_eq!(
            fs::read_to_string(new.join("settings.json")).unwrap(),
            "new"
        );
        assert!(old.join("settings.json").exists());
    }

    #[test]
    fn old_logs_do_not_block_removal() {
        let root = tmp("logs");
        let (old, new) = (root.join("old"), root.join("new"));
        seed(&old);
        fs::create_dir_all(old.join("logs")).unwrap();
        fs::write(old.join("logs/app.log"), "old").unwrap();
        fs::create_dir_all(new.join("logs")).unwrap();
        fs::write(new.join("logs/app.log"), "new").unwrap();
        assert_eq!(
            migrate_dir(&old, &new, &["logs"]).unwrap(),
            Outcome::Moved(4)
        );
        assert!(!old.exists());
        assert_eq!(fs::read_to_string(new.join("logs/app.log")).unwrap(), "new");
    }

    #[test]
    fn resumes_after_an_interrupted_move() {
        let root = tmp("resume");
        let (old, new) = (root.join("old"), root.join("new"));
        seed(&old);
        // Half done: settings already moved by an earlier run.
        fs::create_dir_all(&new).unwrap();
        fs::rename(old.join("settings.json"), new.join("settings.json")).unwrap();
        assert_eq!(migrate_dir(&old, &new, &[]).unwrap(), Outcome::Moved(3));
        assert!(new.join("sidekick.db").exists());
        assert!(!old.exists());
    }

    #[test]
    fn renames_the_database_with_its_side_files() {
        let root = tmp("db");
        fs::write(root.join("sidekick.db"), "db").unwrap();
        fs::write(root.join("sidekick.db-wal"), "wal").unwrap();
        assert!(rename_db(&root, "sidekick.db", "duskie.db").unwrap());
        assert!(root.join("duskie.db").exists());
        assert!(root.join("duskie.db-wal").exists());
        assert!(!root.join("sidekick.db").exists());
        assert!(!rename_db(&root, "sidekick.db", "duskie.db").unwrap());
    }
}
