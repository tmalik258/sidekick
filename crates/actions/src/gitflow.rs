//! Git and GitHub for Ask: what changed (for a commit message or a PR
//! description), what is waiting on you, why CI failed, a PR to review,
//! merged branches to clean and conflicts to hand to an agent. Reading is
//! free; committing and deleting branches are actions behind a tap. GitHub
//! goes through the user's own `gh` CLI and its sign-in.

use std::path::Path;
use std::process::{Command, Stdio};

use crate::dev::{git, hidden, repo};
use crate::{ActionError, Outcome, fail};

/// Diff text past this is cut; a model needs the shape, not every line.
const DIFF_CHARS: usize = 12_000;

fn cut(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    let end = (0..=max)
        .rev()
        .find(|i| text.is_char_boundary(*i))
        .unwrap_or(0);
    format!(
        "{}\n... ({} more characters)",
        &text[..end],
        text.len() - end
    )
}

fn gh(path: Option<&Path>, args: &[&str]) -> Result<String, ActionError> {
    let mut cmd = Command::new("gh");
    if let Some(p) = path {
        cmd.current_dir(p);
    }
    cmd.args(args)
        .env("GH_PROMPT_DISABLED", "1")
        .env("NO_COLOR", "1")
        .stdin(Stdio::null());
    hidden(&mut cmd);
    let out = cmd.output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            ActionError::Failed(
                "GitHub CLI (gh) is not installed. Install it with winget install GitHub.cli, then run gh auth login.".into(),
            )
        } else {
            fail(e)
        }
    })?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(ActionError::Failed(
            err.lines()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("gh failed")
                .trim()
                .to_owned(),
        ))
    }
}

/// The branch work is merged into: origin's default, else main or master.
fn base(path: &Path) -> String {
    if let Ok(head) = git(
        path,
        &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
    ) {
        let head = head.trim();
        if !head.is_empty() {
            return head.to_owned();
        }
    }
    for b in ["main", "master"] {
        if git(path, &["rev-parse", "--verify", "--quiet", b]).is_ok() {
            return b.to_owned();
        }
    }
    "main".into()
}

/// What a commit would hold: the staged changes, or every change when
/// nothing is staged.
pub fn changes(path: &Path) -> Result<String, ActionError> {
    repo(path)?;
    let staged = git(path, &["diff", "--cached", "--stat"])?;
    let (which, stat, diff) = if staged.trim().is_empty() {
        (
            "Nothing is staged; these are all uncommitted changes (a commit would include tracked files).",
            git(path, &["diff", "--stat"])?,
            git(path, &["diff"])?,
        )
    } else {
        ("Staged changes:", staged, git(path, &["diff", "--cached"])?)
    };
    let untracked = git(path, &["ls-files", "--others", "--exclude-standard"])?;
    if stat.trim().is_empty() && untracked.trim().is_empty() {
        return Ok("No changes to commit.".into());
    }
    let mut out = format!("{which}\n{stat}\n{}", cut(&diff, DIFF_CHARS));
    if !untracked.trim().is_empty() {
        out.push_str(&format!(
            "\nNew files not yet added:\n{}",
            cut(&untracked, 1_000)
        ));
    }
    Ok(out)
}

/// This branch against its base: commits and the overall diff, for a PR
/// description.
pub fn branch(path: &Path) -> Result<String, ActionError> {
    repo(path)?;
    let base = base(path);
    let name = git(path, &["branch", "--show-current"])?;
    let range = format!("{base}...HEAD");
    let log = git(
        path,
        &[
            "log",
            "--oneline",
            "--no-decorate",
            &format!("{base}..HEAD"),
        ],
    )?;
    if log.trim().is_empty() {
        return Ok(format!("{} has no commits that {base} lacks.", name.trim()));
    }
    let stat = git(path, &["diff", "--stat", &range])?;
    let diff = git(path, &["diff", &range])?;
    Ok(format!(
        "Branch {} against {base}\nCommits:\n{log}\n{stat}\n{}",
        name.trim(),
        cut(&diff, DIFF_CHARS)
    ))
}

/// Pull requests waiting on the user's review, their own open ones and
/// issues assigned to them, across GitHub.
pub fn waiting() -> Result<String, ActionError> {
    let fields = "number,title,repository,url,updatedAt";
    let review = gh(
        None,
        &[
            "search",
            "prs",
            "--review-requested=@me",
            "--state=open",
            "--json",
            fields,
            "--limit",
            "20",
        ],
    )?;
    let mine = gh(
        None,
        &[
            "search",
            "prs",
            "--author=@me",
            "--state=open",
            "--json",
            fields,
            "--limit",
            "20",
        ],
    )?;
    let issues = gh(
        None,
        &[
            "search",
            "issues",
            "--assignee=@me",
            "--state=open",
            "--json",
            fields,
            "--limit",
            "20",
        ],
    )?;
    Ok(format!(
        "Reviews asked of you:\n{}\nYour open pull requests:\n{}\nIssues assigned to you:\n{}",
        list(&review),
        list(&mine),
        list(&issues)
    ))
}

/// gh search JSON as one line each.
fn list(json: &str) -> String {
    let items: Vec<serde_json::Value> = serde_json::from_str(json).unwrap_or_default();
    if items.is_empty() {
        return "none\n".into();
    }
    items
        .iter()
        .map(|i| {
            format!(
                "- {}#{} {} ({})\n",
                i["repository"]["nameWithOwner"]
                    .as_str()
                    .unwrap_or_default(),
                i["number"],
                i["title"].as_str().unwrap_or_default(),
                i["url"].as_str().unwrap_or_default()
            )
        })
        .collect()
}

/// The latest CI runs here, and the failing log of the newest failure.
pub fn ci(path: &Path) -> Result<String, ActionError> {
    repo(path)?;
    let runs = gh(
        Some(path),
        &[
            "run",
            "list",
            "--limit",
            "6",
            "--json",
            "databaseId,name,status,conclusion,headBranch,url",
        ],
    )?;
    let runs: Vec<serde_json::Value> = serde_json::from_str(&runs).unwrap_or_default();
    if runs.is_empty() {
        return Ok("No CI runs found for this repository.".into());
    }
    let mut out = String::from("Latest runs:\n");
    for r in &runs {
        out.push_str(&format!(
            "- {} on {}: {} {} ({})\n",
            r["name"].as_str().unwrap_or_default(),
            r["headBranch"].as_str().unwrap_or_default(),
            r["status"].as_str().unwrap_or_default(),
            r["conclusion"].as_str().unwrap_or_default(),
            r["url"].as_str().unwrap_or_default()
        ));
    }
    if let Some(failed) = runs.iter().find(|r| r["conclusion"] == "failure") {
        let id = failed["databaseId"].to_string();
        if let Ok(log) = gh(Some(path), &["run", "view", &id, "--log-failed"]) {
            let tail: Vec<&str> = log.lines().rev().take(80).collect();
            let tail: Vec<&str> = tail.into_iter().rev().collect();
            out.push_str(&format!(
                "\nEnd of the failing log:\n{}",
                cut(&tail.join("\n"), 8_000)
            ));
        }
    }
    Ok(out)
}

/// A pull request's description and diff, for a review.
pub fn pr(path: &Path, number: u64) -> Result<String, ActionError> {
    let n = number.to_string();
    let info = gh(
        Some(path),
        &[
            "pr",
            "view",
            &n,
            "--json",
            "title,body,author,baseRefName,headRefName,url",
        ],
    )?;
    let diff = gh(Some(path), &["pr", "diff", &n])?;
    Ok(format!("{info}\n{}", cut(&diff, DIFF_CHARS * 2)))
}

/// Local branches already merged into the base, safe to delete.
pub fn merged(path: &Path) -> Result<Vec<String>, ActionError> {
    repo(path)?;
    let base = base(path);
    let current = git(path, &["branch", "--show-current"])?;
    let short = base.rsplit('/').next().unwrap_or(&base).to_owned();
    let out = git(
        path,
        &["branch", "--merged", &base, "--format=%(refname:short)"],
    )?;
    Ok(out
        .lines()
        .map(str::trim)
        .filter(|b| {
            !b.is_empty()
                && *b != current.trim()
                && *b != short
                && !matches!(*b, "main" | "master" | "develop" | "dev")
        })
        .map(str::to_owned)
        .collect())
}

/// Your commit subjects since midnight, newest first (no merges).
pub fn today_commits(path: &Path) -> Vec<String> {
    if repo(path).is_err() {
        return Vec::new();
    }
    let me = git(path, &["config", "user.email"]).unwrap_or_default();
    let mut args = vec!["log", "--since=midnight", "--no-merges", "--format=%s"];
    let author = format!("--author={}", me.trim());
    if !me.trim().is_empty() {
        args.push(&author);
    }
    git(path, &args)
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Files with merge conflicts right now.
pub fn conflicts(path: &Path) -> Result<Vec<String>, ActionError> {
    repo(path)?;
    let out = git(path, &["diff", "--name-only", "--diff-filter=U"])?;
    Ok(out.lines().map(str::to_owned).collect())
}

/// Commits with `message`: what is staged, or every tracked change when
/// nothing is.
pub fn commit(path: &Path, message: &str) -> Result<Outcome, ActionError> {
    repo(path)?;
    let message = message.trim();
    if message.is_empty() {
        return Err(ActionError::Invalid("the commit message is empty".into()));
    }
    let staged = !git(path, &["diff", "--cached", "--name-only"])?
        .trim()
        .is_empty();
    let args: Vec<&str> = if staged {
        vec!["commit", "-m", message]
    } else {
        vec!["commit", "-a", "-m", message]
    };
    git(path, &args)?;
    let first = message.lines().next().unwrap_or(message);
    Ok(Outcome::msg(format!("Committed: {first}")))
}

// ---- Staging -------------------------------------------------------------------

/// One changed file as the Git dock lists it. A file can be staged and have
/// further changes that are not (both flags set).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct FileChange {
    pub path: String,
    /// `M` changed, `A` added, `D` deleted, `R` renamed, `?` new and untracked, `U` conflict.
    pub kind: char,
    pub staged: bool,
    pub unstaged: bool,
}

/// Reads `git status --porcelain=v1` lines.
pub fn parse_status(text: &str) -> Vec<FileChange> {
    text.lines()
        .filter_map(|line| {
            let mut chars = line.chars();
            let (x, y) = (chars.next()?, chars.next()?);
            let rest = line.get(3..)?;
            // A rename reads "old -> new": the new name is the file.
            let path = rest.rsplit(" -> ").next()?.trim_matches('"').to_owned();
            if x == '?' && y == '?' {
                return Some(FileChange {
                    path,
                    kind: '?',
                    staged: false,
                    unstaged: true,
                });
            }
            let conflict = x == 'U' || y == 'U' || (x == 'A' && y == 'A') || (x == 'D' && y == 'D');
            let kind = if conflict {
                'U'
            } else if x != ' ' {
                x
            } else {
                y
            };
            Some(FileChange {
                path,
                kind,
                staged: x != ' ' && !conflict,
                unstaged: y != ' ',
            })
        })
        .collect()
}

/// Every changed file, for the Changes list.
pub fn status(path: &Path) -> Result<Vec<FileChange>, ActionError> {
    repo(path)?;
    Ok(parse_status(&git(path, &["status", "--porcelain=v1"])?))
}

/// A file name that is safe to hand to git: no option look-alikes, no
/// parent-folder escapes.
fn safe_file(name: &str) -> Result<&str, ActionError> {
    if name.is_empty() || name.starts_with('-') || name.split(['/', '\\']).any(|p| p == "..") {
        return Err(ActionError::Invalid(format!(
            "not a file in this repository: {name}"
        )));
    }
    Ok(name)
}

/// Stages `files`, or everything when the list is empty.
pub fn stage(path: &Path, files: &[String]) -> Result<Outcome, ActionError> {
    repo(path)?;
    if files.is_empty() {
        git(path, &["add", "-A"])?;
        return Ok(Outcome::msg("Staged everything"));
    }
    let mut args = vec!["add", "--"];
    for f in files {
        args.push(safe_file(f)?);
    }
    git(path, &args)?;
    Ok(Outcome::msg(format!("Staged {} file(s)", files.len())))
}

/// Unstages `files`, or everything when the list is empty. Work in the
/// files is never touched.
pub fn unstage(path: &Path, files: &[String]) -> Result<Outcome, ActionError> {
    repo(path)?;
    let mut args = vec!["reset", "-q", "--"];
    for f in files {
        args.push(safe_file(f)?);
    }
    git(path, &args)?;
    Ok(Outcome::msg(if files.is_empty() {
        "Unstaged everything".to_owned()
    } else {
        format!("Unstaged {} file(s)", files.len())
    }))
}

/// Replaces the last commit with what is staged now. A new message is
/// optional; without one the old message stays. The caller warns before a
/// force push is needed (the commit was already pushed).
pub fn amend(path: &Path, message: Option<&str>) -> Result<Outcome, ActionError> {
    repo(path)?;
    match message.map(str::trim).filter(|m| !m.is_empty()) {
        Some(m) => git(path, &["commit", "--amend", "-m", m])?,
        None => git(path, &["commit", "--amend", "--no-edit"])?,
    };
    Ok(Outcome::msg("Amended the last commit"))
}

/// Whether the last commit is already on a remote branch, so amending it
/// would need a force push.
pub fn last_commit_pushed(path: &Path) -> bool {
    git(path, &["branch", "-r", "--contains", "HEAD"])
        .map(|o| !o.trim().is_empty())
        .unwrap_or(false)
}

/// What a model reads to write a commit message: the staged diff when there
/// is one (else every tracked change), cut to `cap` characters, with how
/// many files exist and how many the text still covers.
pub fn diff_for_message(path: &Path, cap: usize) -> Result<(String, usize, usize), ActionError> {
    repo(path)?;
    let staged = !git(path, &["diff", "--cached", "--name-only"])?
        .trim()
        .is_empty();
    let args: &[&str] = if staged {
        &["diff", "--cached"]
    } else {
        &["diff", "HEAD"]
    };
    let full = git(path, args)?;
    let total = full
        .lines()
        .filter(|l| l.starts_with("diff --git "))
        .count();
    let text = cut(&full, cap);
    let covered = text
        .lines()
        .filter(|l| l.starts_with("diff --git "))
        .count();
    Ok((text, covered, total))
}

/// Deletes merged branches (`git branch -d`, which refuses unmerged ones).
pub fn delete_branches(path: &Path, branches: &[String]) -> Result<Outcome, ActionError> {
    repo(path)?;
    let allowed = merged(path)?;
    let mut gone = Vec::new();
    for b in branches.iter().filter(|b| allowed.contains(b)) {
        git(path, &["branch", "-d", b])?;
        gone.push(b.clone());
    }
    Ok(Outcome::msg(match gone.len() {
        0 => "No merged branches to delete".into(),
        1 => format!("Deleted {}", gone[0]),
        n => format!("Deleted {n} merged branches"),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .unwrap();
        assert!(
            ok.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&ok.stderr)
        );
    }

    #[test]
    fn commits_lists_merged_branches_and_cleans_them() {
        let dir = std::env::temp_dir().join(format!("sidekick-gitflow-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        run(&dir, &["init", "-q", "-b", "main"]);
        run(&dir, &["config", "user.email", "t@example.com"]);
        run(&dir, &["config", "user.name", "T"]);
        std::fs::write(dir.join("a.txt"), "one\n").unwrap();
        run(&dir, &["add", "."]);
        run(&dir, &["commit", "-q", "-m", "first"]);
        assert_eq!(changes(&dir).unwrap(), "No changes to commit.");

        std::fs::write(dir.join("a.txt"), "two\n").unwrap();
        assert!(changes(&dir).unwrap().contains("+two"));
        commit(&dir, "Change a\n\nbody").unwrap();
        assert!(changes(&dir).unwrap().contains("No changes"));

        run(&dir, &["branch", "done-feature"]);
        run(&dir, &["checkout", "-q", "-b", "open-feature"]);
        std::fs::write(dir.join("b.txt"), "new\n").unwrap();
        run(&dir, &["add", "."]);
        run(&dir, &["commit", "-q", "-m", "wip"]);
        assert!(branch(&dir).unwrap().contains("wip"));
        run(&dir, &["checkout", "-q", "main"]);

        assert_eq!(merged(&dir).unwrap(), ["done-feature"]);
        assert!(conflicts(&dir).unwrap().is_empty());
        // An unmerged branch is never deleted, even when asked.
        delete_branches(&dir, &["done-feature".into(), "open-feature".into()]).unwrap();
        let left = git(&dir, &["branch", "--format=%(refname:short)"]).unwrap();
        assert!(left.contains("open-feature") && !left.contains("done-feature"));
        let _ = std::fs::remove_dir_all(dir);
    }

    fn fresh(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("duskie-stage-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        run(&dir, &["init", "-q", "-b", "main"]);
        run(&dir, &["config", "user.email", "t@example.com"]);
        run(&dir, &["config", "user.name", "T"]);
        std::fs::write(dir.join("a.txt"), "one\n").unwrap();
        run(&dir, &["add", "."]);
        run(&dir, &["commit", "-q", "-m", "first"]);
        dir
    }

    #[test]
    fn status_lines_become_changes() {
        let list = parse_status(
            " M a.txt\nM  b.txt\nMM c.txt\n?? d.txt\nR  old.txt -> new.txt\nUU e.txt\n",
        );
        let by = |p: &str| list.iter().find(|f| f.path == p).unwrap().clone();
        assert_eq!((by("a.txt").staged, by("a.txt").unstaged), (false, true));
        assert_eq!((by("b.txt").staged, by("b.txt").unstaged), (true, false));
        assert_eq!((by("c.txt").staged, by("c.txt").unstaged), (true, true));
        assert_eq!(by("d.txt").kind, '?');
        assert_eq!(by("new.txt").kind, 'R');
        assert_eq!(by("e.txt").kind, 'U');
        assert!(!by("e.txt").staged);
    }

    #[test]
    fn stage_unstage_and_commit_only_what_is_staged() {
        let dir = fresh("flow");
        std::fs::write(dir.join("a.txt"), "two\n").unwrap();
        std::fs::write(dir.join("b.txt"), "new\n").unwrap();
        stage(&dir, &["a.txt".into()]).unwrap();
        let st = status(&dir).unwrap();
        assert!(st.iter().find(|f| f.path == "a.txt").unwrap().staged);
        assert!(!st.iter().find(|f| f.path == "b.txt").unwrap().staged);
        // Unstaging leaves the edit in the file.
        unstage(&dir, &["a.txt".into()]).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "two\n");
        assert!(status(&dir).unwrap().iter().all(|f| !f.staged));
        stage(&dir, &[]).unwrap();
        assert!(status(&dir).unwrap().iter().all(|f| f.staged));
        unstage(&dir, &[]).unwrap();
        stage(&dir, &["a.txt".into()]).unwrap();
        commit(&dir, "Change a").unwrap();
        // b.txt was never staged, so it is still waiting.
        let left = status(&dir).unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].path, "b.txt");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn amend_folds_staged_work_into_the_last_commit() {
        let dir = fresh("amend");
        std::fs::write(dir.join("a.txt"), "two\n").unwrap();
        stage(&dir, &[]).unwrap();
        amend(&dir, None).unwrap();
        let count = git(&dir, &["rev-list", "--count", "HEAD"]).unwrap();
        assert_eq!(count.trim(), "1", "amend does not add a commit");
        assert_eq!(
            git(&dir, &["log", "-1", "--format=%s"]).unwrap().trim(),
            "first"
        );
        amend(&dir, Some("Better message")).unwrap();
        assert_eq!(
            git(&dir, &["log", "-1", "--format=%s"]).unwrap().trim(),
            "Better message"
        );
        assert!(
            !last_commit_pushed(&dir),
            "no remote, so nothing was pushed"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn odd_file_names_are_refused() {
        let dir = fresh("names");
        for bad in ["--all", "../x", "a/../../x", ""] {
            assert!(stage(&dir, &[bad.into()]).is_err(), "{bad}");
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_message_diff_is_capped_and_says_how_much_it_covers() {
        let dir = fresh("cap");
        for n in 0..5 {
            std::fs::write(dir.join(format!("f{n}.txt")), "x".repeat(400)).unwrap();
        }
        stage(&dir, &[]).unwrap();
        let (text, covered, total) = diff_for_message(&dir, 700).unwrap();
        assert_eq!(total, 5);
        assert!(covered < total && covered >= 1, "covered {covered}");
        assert!(text.len() < 900);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn long_diffs_are_cut_on_a_character() {
        let s = "é".repeat(10);
        assert!(cut(&s, 5).starts_with("éé"));
        assert_eq!(cut("short", 50), "short");
    }
}
