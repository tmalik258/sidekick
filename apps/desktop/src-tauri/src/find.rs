//! Finding files and folders on disk by name, for "find my invoice" or
//! "where is the sidekick folder": the user folders, code folders and app
//! data, within a time budget so an answer never waits long.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// Folders never worth walking into: huge, generated or system.
const SKIP: &[&str] = &[
    "node_modules",
    ".git",
    "target",
    ".next",
    "dist",
    "build",
    "__pycache__",
    ".venv",
    "venv",
    ".cache",
    "cache",
    "temp",
    "tmp",
    "$recycle.bin",
    "windows",
    "program files",
    "program files (x86)",
    "packages",
];
const MAX_DEPTH: usize = 7;
pub const BUDGET: Duration = Duration::from_millis(2500);
pub const MAX_RESULTS: usize = 15;

#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub path: PathBuf,
    pub folder: bool,
    pub modified: Option<SystemTime>,
}

/// Every word of the query is in the name (case and separators ignored).
pub fn matches(name: &str, words: &[String]) -> bool {
    let n = name.to_lowercase().replace(['_', '-', '.'], " ");
    !words.is_empty() && words.iter().all(|w| n.contains(w.as_str()))
}

pub fn words(query: &str) -> Vec<String> {
    query
        .to_lowercase()
        .replace(['_', '-', '.'], " ")
        .split_whitespace()
        .filter(|w| !["file", "files", "folder", "folders", "my", "the", "a"].contains(w))
        .map(str::to_owned)
        .collect()
}

/// Walks `roots` breadth first (so near matches come first) for names that
/// match. `folders`: Some(true) only folders, Some(false) only files.
pub fn find(roots: &[PathBuf], query: &str, folders: Option<bool>, budget: Duration) -> Vec<Found> {
    let words = words(query);
    if words.is_empty() {
        return Vec::new();
    }
    let started = Instant::now();
    let mut out: Vec<Found> = Vec::new();
    let mut queue: std::collections::VecDeque<(PathBuf, usize)> = roots
        .iter()
        .filter(|r| r.is_dir())
        .map(|r| (r.clone(), 0))
        .collect();
    let mut seen = std::collections::HashSet::new();
    while let Some((dir, depth)) = queue.pop_front() {
        if started.elapsed() > budget || out.len() >= MAX_RESULTS {
            break;
        }
        if !seen.insert(dir.clone()) {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            // Links can loop; real folders only.
            let is_dir = kind.is_dir();
            if matches(&name, &words) && folders.is_none_or(|f| f == is_dir) {
                out.push(Found {
                    path: entry.path(),
                    folder: is_dir,
                    modified: entry.metadata().ok().and_then(|m| m.modified().ok()),
                });
                if out.len() >= MAX_RESULTS {
                    break;
                }
            }
            if is_dir
                && depth + 1 < MAX_DEPTH
                && !name.starts_with('.')
                && !SKIP.contains(&name.to_lowercase().as_str())
            {
                queue.push_back((entry.path(), depth + 1));
            }
        }
    }
    out
}

/// Where to look: the user's own folders, code folders and app data.
pub fn roots(home: &Path, code_folders: &[PathBuf]) -> Vec<PathBuf> {
    let mut r: Vec<PathBuf> = [
        "Desktop",
        "Documents",
        "Downloads",
        "Pictures",
        "Videos",
        "Music",
        "OneDrive",
    ]
    .iter()
    .map(|d| home.join(d))
    .collect();
    r.extend(code_folders.iter().cloned());
    r.push(home.join("AppData").join("Roaming"));
    r.push(home.join("AppData").join("Local"));
    // Last: the rest of the home folder (projects kept at the top level).
    r.push(home.to_path_buf());
    r
}

/// The answer the model reads: one line per hit.
pub fn describe(found: &[Found], query: &str) -> String {
    if found.is_empty() {
        return format!(
            "No file or folder named like \"{query}\" in the user's folders, code folders or app data."
        );
    }
    found
        .iter()
        .map(|f| {
            let when = f
                .modified
                .map(|m| {
                    chrono::DateTime::<chrono::Local>::from(m)
                        .format("%Y-%m-%d")
                        .to_string()
                })
                .unwrap_or_default();
            format!(
                "- {} {} (changed {when})",
                if f.folder { "folder" } else { "file" },
                f.path.display()
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_by_every_word_nearest_first() {
        let root = std::env::temp_dir().join(format!("sidekick-find-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("work/node_modules/sidekick-x")).unwrap();
        std::fs::create_dir_all(root.join("work/deep/er")).unwrap();
        std::fs::create_dir_all(root.join("dev.sidekick.app")).unwrap();
        std::fs::write(root.join("work/deep/er/Sidekick_Notes.md"), "x").unwrap();
        std::fs::write(root.join("work/invoice-sept.pdf"), "x").unwrap();

        let hits = find(std::slice::from_ref(&root), "sidekick", None, BUDGET);
        let names: Vec<String> = hits
            .iter()
            .map(|h| h.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            ["dev.sidekick.app", "Sidekick_Notes.md"],
            "nearest first, node_modules skipped"
        );
        assert!(hits[0].folder);

        let only_files = find(
            std::slice::from_ref(&root),
            "sidekick files",
            Some(false),
            BUDGET,
        );
        assert_eq!(only_files.len(), 1);
        assert_eq!(
            find(std::slice::from_ref(&root), "invoice sept", None, BUDGET).len(),
            1
        );
        assert!(find(std::slice::from_ref(&root), "the files", None, BUDGET).is_empty());
        assert!(describe(&[], "x").starts_with("No file"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
