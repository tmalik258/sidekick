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

    #[test]
    fn long_diffs_are_cut_on_a_character() {
        let s = "é".repeat(10);
        assert!(cut(&s, 5).starts_with("éé"));
        assert_eq!(cut("short", 50), "short");
    }
}
