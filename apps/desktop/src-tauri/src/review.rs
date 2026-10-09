//! Keep or undo what a coding agent changed. Before a session starts, the
//! project's state is snapshotted (`git stash create`, which touches
//! nothing); afterwards every change since then is listed by file and by
//! hunk, and each hunk, file or the whole lot can be put back.
//!
//! A project that is not a git repository gets a private one instead, kept
//! in Sidekick's data folder (`--git-dir` with the project as work tree), so
//! the same review, undo and rewind work and the project itself is never
//! touched.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

/// The project as it was when the agent started.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Baseline {
    pub root: PathBuf,
    /// A commit holding the working tree at the start (or HEAD when it was
    /// clean).
    pub commit: String,
    /// Files git did not track at the start; new ones since are the agent's.
    pub untracked: Vec<String>,
    /// The private repository for a project without git, if it is one.
    #[serde(default)]
    pub git_dir: Option<PathBuf>,
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
    git_in(root, None, args)
}

/// Runs git in `root`, against a private repository when `git_dir` is set.
fn git_in(root: &Path, git_dir: Option<&Path>, args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new("git");
    if let Some(dir) = git_dir {
        cmd.arg("--git-dir").arg(dir).arg("--work-tree").arg(root);
    }
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

/// git for a snapshot: in its project, against its private repository if any.
fn run(b: &Baseline, args: &[&str]) -> Result<String, String> {
    git_in(&b.root, b.git_dir.as_deref(), args)
}

/// Like `run`, for output that may not be text (a file's stored bytes).
fn run_bytes(b: &Baseline, args: &[&str]) -> Result<Vec<u8>, String> {
    let mut cmd = Command::new("git");
    if let Some(dir) = &b.git_dir {
        cmd.arg("--git-dir")
            .arg(dir)
            .arg("--work-tree")
            .arg(&b.root);
    }
    cmd.args(args).current_dir(&b.root);
    hide_console(&mut cmd);
    let out = cmd
        .output()
        .map_err(|e| format!("could not run git: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_owned());
    }
    Ok(out.stdout)
}

#[cfg(windows)]
fn hide_console(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_console(_cmd: &mut Command) {}

/// A fingerprint of a file's bytes (FNV-1a), stable across runs, to tell
/// whether it changed since; None when the file is gone.
pub fn fingerprint(root: &Path, path: &str) -> Option<u64> {
    let bytes = std::fs::read(root.join(path)).ok()?;
    Some(bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
    }))
}

/// Folders a private snapshot leaves out: rebuilt by tools, often huge.
const SHADOW_SKIP: &[&str] = &[
    "node_modules/",
    "target/",
    ".venv/",
    "venv/",
    "__pycache__/",
    "dist/",
    "build/",
    ".next/",
    "out/",
    "bin/",
    "obj/",
    ".gradle/",
    ".idea/",
    ".vs/",
];
/// More files than this and a project without git is not snapshotted.
const SHADOW_MAX_FILES: usize = 20_000;

/// Snapshots `dir`: with its own git when it is a repository, otherwise with
/// a private repository under `shadow_root`. None when neither works (no
/// git installed, or a project too big to copy).
pub fn snapshot(dir: &Path, shadow_root: Option<&Path>) -> Option<Baseline> {
    if git(dir, &["rev-parse", "--show-toplevel"]).is_err() {
        return shadow(dir, shadow_root?);
    }
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
        git_dir: None,
    })
}

/// A private repository for a project without git: one per folder, each
/// snapshot a commit in it.
fn shadow(dir: &Path, shadow_root: &Path) -> Option<Baseline> {
    let root = dir.canonicalize().ok()?;
    let key = root
        .to_string_lossy()
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
        });
    let git_dir = shadow_root.join(format!("{key:016x}.git"));
    let b = Baseline {
        root,
        commit: String::new(),
        untracked: Vec::new(),
        git_dir: Some(git_dir.clone()),
    };
    if !git_dir.join("HEAD").exists() {
        std::fs::create_dir_all(&git_dir).ok()?;
        run(&b, &["init", "-q"]).ok()?;
        // Files are stored as they are; Windows' autocrlf never applies here.
        run(&b, &["config", "core.autocrlf", "false"]).ok()?;
        let exclude = git_dir.join("info").join("exclude");
        std::fs::create_dir_all(exclude.parent()?).ok()?;
        std::fs::write(&exclude, SHADOW_SKIP.join("\n")).ok()?;
    }
    let pending = run(&b, &["ls-files", "--others", "--exclude-standard"]).ok()?;
    if pending.lines().count() > SHADOW_MAX_FILES {
        return None;
    }
    run(&b, &["add", "-A"]).ok()?;
    run(
        &b,
        &[
            "-c",
            "user.name=Sidekick",
            "-c",
            "user.email=sidekick@localhost",
            "-c",
            "core.autocrlf=false",
            "commit",
            "-q",
            "--allow-empty",
            "--no-verify",
            "-m",
            "snapshot",
        ],
    )
    .ok()?;
    let commit = run(&b, &["rev-parse", "HEAD"]).ok()?.trim().to_owned();
    Some(Baseline { commit, ..b })
}

fn untracked(root: &Path) -> Vec<String> {
    untracked_in(root, None)
}

fn untracked_in(root: &Path, git_dir: Option<&Path>) -> Vec<String> {
    git_in(
        root,
        git_dir,
        &["ls-files", "--others", "--exclude-standard"],
    )
    .map(|s| s.lines().map(str::to_owned).collect())
    .unwrap_or_default()
}

/// Everything changed since the snapshot, by file.
pub fn changes(b: &Baseline) -> Result<Vec<FileChange>, String> {
    let diff = run(
        b,
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
    for path in untracked_in(&b.root, b.git_dir.as_deref()) {
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

/// Puts one hunk back as it was. Done here rather than with `git apply`,
/// which rewrites line endings on Windows (core.autocrlf); the file keeps its own.
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
    let full = b.root.join(path);
    let text = std::fs::read_to_string(&full).map_err(|e| format!("Could not read {path}: {e}"))?;
    let undone = reverse_hunk(&text, hunk)
        .ok_or("Could not undo that change (the file changed again since).")?;
    std::fs::write(&full, undone).map_err(|e| format!("Could not write {path}: {e}"))
}

/// The text with one hunk taken back out, or None when its lines are gone.
fn reverse_hunk(text: &str, hunk: &Hunk) -> Option<String> {
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let ends_with_eol = text.ends_with('\n');
    let mut lines: Vec<&str> = text.lines().collect();
    let (mut now, mut before) = (Vec::new(), Vec::new());
    for l in &hunk.lines {
        match l.split_at_checked(1) {
            Some((" ", rest)) => {
                now.push(rest);
                before.push(rest);
            }
            Some(("+", rest)) => now.push(rest),
            Some(("-", rest)) => before.push(rest),
            _ => {}
        }
    }
    // Where the hunk says it starts in the current file ("+c,d").
    let start = hunk
        .header
        .split_whitespace()
        .find_map(|p| p.strip_prefix('+'))
        .and_then(|p| p.split(',').next())
        .and_then(|n| n.parse::<usize>().ok())
        .map_or(0, |n| n.saturating_sub(1));
    let matches = |at: usize| {
        lines
            .get(at..at + now.len())
            .is_some_and(|w| w == now.as_slice())
    };
    // Its own place first, else the nearest place the same lines are.
    let at = if matches(start) {
        start
    } else {
        (0..=lines.len().saturating_sub(now.len()))
            .filter(|&i| matches(i))
            .min_by_key(|&i| i.abs_diff(start))?
    };
    lines.splice(at..at + now.len(), before);
    let mut out = lines.join(eol);
    if ends_with_eol && !out.is_empty() {
        out.push_str(eol);
    }
    Some(out)
}

/// Puts a whole file back; a file the agent created goes to the Recycle Bin.
pub fn undo_file(b: &Baseline, path: &str) -> Result<(), String> {
    if path.contains("..") {
        return Err("That path is outside the project.".into());
    }
    let full = b.root.join(path);
    let existed = run(b, &["cat-file", "-e", &format!("{}:{path}", b.commit)]).is_ok();
    if existed {
        if !full.exists() {
            run(b, &["checkout", &b.commit, "--", path])?;
            return Ok(());
        }
        // The stored copy has LF endings; a CRLF file gets its CRLF back.
        let mut old = run_bytes(b, &["show", &format!("{}:{path}", b.commit)])?;
        let crlf = std::fs::read(&full).is_ok_and(|now| now.windows(2).any(|w| w == b"\r\n"));
        if crlf && !old.windows(2).any(|w| w == b"\r\n") {
            old = String::from_utf8(old)
                .map(|t| t.replace('\n', "\r\n").into_bytes())
                .unwrap_or_else(|e| e.into_bytes());
        }
        std::fs::write(&full, old).map_err(|e| format!("Could not write {path}: {e}"))?;
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
        let b = snapshot(&dir, None).expect("a repo");

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
    fn reverses_a_hunk_and_keeps_the_files_line_endings() {
        let hunk = Hunk {
            header: "@@ -1,3 +1,3 @@".into(),
            lines: vec![" a".into(), "-b".into(), "+B".into(), " c".into()],
        };
        assert_eq!(
            reverse_hunk("a\r\nB\r\nc\r\nd\r\n", &hunk).unwrap(),
            "a\r\nb\r\nc\r\nd\r\n"
        );
        assert_eq!(reverse_hunk("a\nB\nc", &hunk).unwrap(), "a\nb\nc");
        // Moved down by an edit above it: found nearby.
        assert_eq!(reverse_hunk("x\na\nB\nc\n", &hunk).unwrap(), "x\na\nb\nc\n");
        // Its lines are gone: nothing to undo.
        assert!(reverse_hunk("a\nb\nc\n", &hunk).is_none());
    }

    #[test]
    fn refuses_paths_outside_the_project() {
        let b = Baseline {
            root: PathBuf::from("."),
            commit: "HEAD".into(),
            untracked: Vec::new(),
            git_dir: None,
        };
        assert!(undo_file(&b, "../secret").is_err());
    }

    #[test]
    fn tracks_a_project_without_git_privately() {
        if std::process::Command::new("git")
            .arg("--version")
            .output()
            .is_err()
        {
            return;
        }
        let base = std::env::temp_dir().join(format!("sidekick-nogit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let dir = base.join("project");
        let shadows = base.join("snapshots");
        std::fs::create_dir_all(dir.join("node_modules")).unwrap();
        std::fs::write(dir.join("notes.txt"), "one\ntwo\n").unwrap();
        std::fs::write(dir.join("node_modules").join("big.js"), "x").unwrap();
        let b = snapshot(&dir, Some(&shadows)).expect("a private snapshot");
        assert!(b.git_dir.is_some());
        assert!(!dir.join(".git").exists(), "the project is never touched");
        std::fs::write(dir.join("notes.txt"), "one\nTWO\n").unwrap();
        std::fs::write(dir.join("new.txt"), "hi\n").unwrap();
        let files = changes(&b).unwrap();
        let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["new.txt", "notes.txt"], "node_modules is left out");
        assert_eq!(undo_all(&b).unwrap(), 2);
        assert_eq!(
            std::fs::read_to_string(dir.join("notes.txt")).unwrap(),
            "one\ntwo\n"
        );
        assert!(!dir.join("new.txt").exists());
        let _ = std::fs::remove_dir_all(base);
    }
}
