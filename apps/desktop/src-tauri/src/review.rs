//! Keep or undo what a coding agent changed. Before a session starts, the
//! project's state is snapshotted (`git stash create`, which touches
//! nothing); afterwards every change since then is listed by file and by
//! hunk, and each hunk, file or the whole lot can be put back.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

/// The project as it was when the agent started.
#[derive(Debug, Clone)]
pub struct Baseline {
    pub root: PathBuf,
    /// A commit holding the working tree at the start (or HEAD when it was
    /// clean).
    pub commit: String,
    /// Files git did not track at the start; new ones since are the agent's.
    pub untracked: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileChange {
    pub path: String,
    /// "added", "deleted" or "modified".
    pub status: &'static str,
    pub added: usize,
    pub removed: usize,
    pub hunks: Vec<Hunk>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hunk {
    /// The `@@ ... @@` line.
    pub header: String,
    /// The hunk's lines, each starting with ' ', '+' or '-'.
    pub lines: Vec<String>,
}

fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new("git");
    cmd.args(args).current_dir(root);
    hide_console(&mut cmd);
    let out = cmd
        .output()
        .map_err(|e| format!("could not run git: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_owned());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(windows)]
fn hide_console(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_console(_cmd: &mut Command) {}

/// Snapshots `dir` when it is in a git repository; None otherwise.
pub fn snapshot(dir: &Path) -> Option<Baseline> {
    let root = PathBuf::from(git(dir, &["rev-parse", "--show-toplevel"]).ok()?.trim());
    // A commit with the uncommitted work, or nothing when the tree is clean.
    let stash = git(&root, &["stash", "create"]).ok()?.trim().to_owned();
    let commit = if stash.is_empty() {
        git(&root, &["rev-parse", "HEAD"]).ok()?.trim().to_owned()
    } else {
        stash
    };
    Some(Baseline {
        untracked: untracked(&root),
        root,
        commit,
    })
}

fn untracked(root: &Path) -> Vec<String> {
    git(root, &["ls-files", "--others", "--exclude-standard"])
        .map(|s| s.lines().map(str::to_owned).collect())
        .unwrap_or_default()
}

/// Everything changed since the snapshot, by file.
pub fn changes(b: &Baseline) -> Result<Vec<FileChange>, String> {
    let diff = git(
        &b.root,
        &[
            "diff",
            "--no-color",
            "--no-ext-diff",
            "-U3",
            &b.commit,
            "--",
        ],
    )?;
    let mut files = parse(&diff);
    for path in untracked(&b.root) {
        if b.untracked.contains(&path) {
            continue;
        }
        let text = std::fs::read_to_string(b.root.join(&path)).unwrap_or_default();
        let lines: Vec<String> = text.lines().map(|l| format!("+{l}")).collect();
        files.push(FileChange {
            path,
            status: "added",
            added: lines.len(),
            removed: 0,
            hunks: vec![Hunk {
                header: format!("@@ -0,0 +1,{} @@", lines.len()),
                lines,
            }],
        });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

/// Splits a unified diff into files and hunks.
pub fn parse(diff: &str) -> Vec<FileChange> {
    let mut files: Vec<FileChange> = Vec::new();
    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            let path = rest.rsplit_once(" b/").map_or(rest, |(_, p)| p).to_owned();
            files.push(FileChange {
                path,
                status: "modified",
                added: 0,
                removed: 0,
                hunks: Vec::new(),
            });
            continue;
        }
        let Some(file) = files.last_mut() else {
            continue;
        };
        if line.starts_with("new file mode") {
            file.status = "added";
        } else if line.starts_with("deleted file mode") {
            file.status = "deleted";
        } else if line.starts_with("@@") {
            file.hunks.push(Hunk {
                header: line.to_owned(),
                lines: Vec::new(),
            });
        } else if let Some(hunk) = file.hunks.last_mut()
            && (line.starts_with(['+', '-', ' ']) || line.starts_with('\\'))
            && !line.starts_with("+++")
            && !line.starts_with("---")
        {
            if line.starts_with('+') {
                file.added += 1;
            } else if line.starts_with('-') {
                file.removed += 1;
            }
            hunk.lines.push(line.to_owned());
        }
    }
    files
}

/// Puts one hunk back as it was.
pub fn undo_hunk(b: &Baseline, path: &str, index: usize) -> Result<(), String> {
    let files = changes(b)?;
    let file = files
        .iter()
        .find(|f| f.path == path)
        .ok_or("That file has no changes any more.")?;
    if file.status != "modified" || file.hunks.len() == 1 {
        return undo_file(b, path);
    }
    let hunk = file
        .hunks
        .get(index)
        .ok_or("That change is not there any more.")?;
    let patch = format!(
        "diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n{}\n{}\n",
        hunk.header,
        hunk.lines.join("\n")
    );
    let tmp = std::env::temp_dir().join(format!("sidekick-undo-{}.patch", std::process::id()));
    std::fs::write(&tmp, patch).map_err(|e| e.to_string())?;
    let tmp_s = tmp.to_string_lossy().into_owned();
    let result = git(&b.root, &["apply", "-R", "--whitespace=nowarn", &tmp_s]);
    let _ = std::fs::remove_file(&tmp);
    result
        .map(|_| ())
        .map_err(|e| format!("Could not undo that change (the file changed again since): {e}"))
}

/// Puts a whole file back; a file the agent created goes to the Recycle Bin.
pub fn undo_file(b: &Baseline, path: &str) -> Result<(), String> {
    if path.contains("..") {
        return Err("That path is outside the project.".into());
    }
    let full = b.root.join(path);
    let existed = git(
        &b.root,
        &["cat-file", "-e", &format!("{}:{path}", b.commit)],
    )
    .is_ok();
    if existed {
        git(&b.root, &["checkout", &b.commit, "--", path])?;
        return Ok(());
    }
    if full.exists() {
        trash::delete(&full).map_err(|e| format!("Could not remove {path}: {e}"))?;
    }
    Ok(())
}

/// Puts everything back.
pub fn undo_all(b: &Baseline) -> Result<usize, String> {
    let files = changes(b)?;
    for f in &files {
        undo_file(b, &f.path)?;
    }
    Ok(files.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIFF: &str = "diff --git a/src/a.rs b/src/a.rs
index 1..2 100644
--- a/src/a.rs
+++ b/src/a.rs
@@ -1,3 +1,3 @@
 fn a() {
-    old();
+    new();
 }
@@ -10,2 +10,3 @@
 x
+y
 z
diff --git a/b.txt b/b.txt
new file mode 100644
--- /dev/null
+++ b/b.txt
@@ -0,0 +1 @@
+hi
";

    #[test]
    fn splits_a_diff_into_files_and_hunks() {
        let files = parse(DIFF);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, "src/a.rs");
        assert_eq!(files[0].hunks.len(), 2);
        assert_eq!((files[0].added, files[0].removed), (2, 1));
        assert_eq!(files[0].hunks[1].lines, [" x", "+y", " z"]);
        assert_eq!(files[1].status, "added");
    }

    /// A throwaway repo with one committed file.
    fn repo(name: &str) -> Option<PathBuf> {
        let dir =
            std::env::temp_dir().join(format!("sidekick-review-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).ok()?;
        let run = |args: &[&str]| git(&dir, args).ok();
        run(&["init", "-q"])?;
        run(&["config", "user.email", "t@example.com"])?;
        run(&["config", "user.name", "t"])?;
        let body: String = (1..=30).map(|n| format!("line {n}\n")).collect();
        std::fs::write(dir.join("a.txt"), body).ok()?;
        run(&["add", "."])?;
        run(&["commit", "-q", "-m", "start"])?;
        Some(dir)
    }

    #[test]
    fn undoes_one_hunk_a_file_or_everything() {
        let Some(dir) = repo("undo") else {
            return; // no git here
        };
        // Work in progress before the agent: kept no matter what.
        let mut text = std::fs::read_to_string(dir.join("a.txt")).unwrap();
        text = text.replace("line 30\n", "line 30 (mine)\n");
        std::fs::write(dir.join("a.txt"), &text).unwrap();
        let b = snapshot(&dir).expect("a repo");

        // The agent edits near the top and the middle, and adds a file.
        let edited = text
            .replace("line 2\n", "line 2 changed\n")
            .replace("line 15\n", "line 15 changed\n");
        std::fs::write(dir.join("a.txt"), edited).unwrap();
        std::fs::write(dir.join("new.txt"), "hello\n").unwrap();

        let files = changes(&b).unwrap();
        let a = files.iter().find(|f| f.path == "a.txt").unwrap();
        assert_eq!(a.hunks.len(), 2, "{a:?}");
        assert!(
            files
                .iter()
                .any(|f| f.path == "new.txt" && f.status == "added")
        );

        undo_hunk(&b, "a.txt", 0).unwrap();
        let now = std::fs::read_to_string(dir.join("a.txt")).unwrap();
        assert!(now.contains("line 2\n") && now.contains("line 15 changed\n"));
        assert!(now.contains("line 30 (mine)"), "earlier work stays");

        undo_all(&b).unwrap();
        let now = std::fs::read_to_string(dir.join("a.txt")).unwrap();
        assert_eq!(now, text);
        assert!(!dir.join("new.txt").exists());
        assert!(changes(&b).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn refuses_paths_outside_the_project() {
        let b = Baseline {
            root: PathBuf::from("."),
            commit: "HEAD".into(),
            untracked: Vec::new(),
        };
        assert!(undo_file(&b, "../secret").is_err());
    }
}
