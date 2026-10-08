//! Developer actions: pull, install dependencies,
//! create `.env` from its example, start Docker Desktop, and open a project.
//! Every command is a fixed program with argument lists; paths are only
//! ever passed as arguments, never through a shell.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::{ActionError, Outcome, fail, system};

pub(crate) fn hidden(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

pub(crate) fn repo(path: &Path) -> Result<(), ActionError> {
    if path.join(".git").exists() {
        Ok(())
    } else {
        Err(ActionError::Invalid(format!(
            "{} is not a git repository",
            path.display()
        )))
    }
}

pub(crate) fn git(path: &Path, args: &[&str]) -> Result<String, ActionError> {
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(path)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null());
    hidden(&mut cmd);
    let out = cmd.output().map_err(fail)?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(ActionError::Failed(
            err.lines().last().unwrap_or("git failed").trim().to_owned(),
        ))
    }
}

/// Lockfiles and the command that installs from each.
const MANAGERS: &[(&str, &str, &[&str])] = &[
    ("pnpm-lock.yaml", "pnpm", &["install"]),
    ("package-lock.json", "npm", &["install"]),
    ("yarn.lock", "yarn", &["install"]),
    ("bun.lockb", "bun", &["install"]),
    ("uv.lock", "uv", &["sync"]),
    ("poetry.lock", "poetry", &["install"]),
    ("Cargo.lock", "cargo", &["fetch"]),
];

pub fn lockfiles_changed(files: &str) -> bool {
    files
        .lines()
        .any(|f| MANAGERS.iter().any(|(lock, _, _)| f.trim().ends_with(lock)))
}

/// Marks an undo record that resets a repo back to a commit.
pub const RESET_TO: &str = "git-reset:";

/// Fast-forward pull; says when dependencies changed. With `stash`, your
/// uncommitted changes are put aside first and put back after. Never
/// merges: when the branches have split it says so.
pub fn pull(path: &Path, stash: bool) -> Result<Outcome, ActionError> {
    repo(path)?;
    let dirty = !git(path, &["status", "--porcelain", "--untracked-files=no"])?
        .trim()
        .is_empty();
    if dirty && !stash {
        return Err(ActionError::Failed(
            "You have uncommitted changes. Stash, pull, put back keeps them.".into(),
        ));
    }
    let before = git(path, &["rev-parse", "HEAD"])?;
    let _ = git(path, &["fetch", "--quiet", "--no-tags"]);
    let ahead = git(path, &["rev-list", "--count", "@{u}..HEAD"]).unwrap_or_default();
    if ahead.trim().parse::<u64>().unwrap_or(0) > 0 {
        return Err(ActionError::Failed(
            "Your branch and the remote have split. Open it in your editor to merge.".into(),
        ));
    }
    if dirty {
        git(path, &["stash", "push", "--quiet", "-m", "sidekick: pull"])?;
    }
    let pulled = git(path, &["pull", "--ff-only", "--quiet"]);
    if dirty {
        // Put the changes back even when the pull failed.
        if let Err(e) = git(path, &["stash", "pop", "--quiet"]) {
            return Err(ActionError::Failed(format!(
                "Pulled, but your changes clash with the new commits; they are kept in git stash ({e})"
            )));
        }
    }
    pulled?;
    let after = git(path, &["rev-parse", "HEAD"])?;
    if before.trim() == after.trim() {
        return Ok(Outcome::msg("Already up to date"));
    }
    let changed = git(path, &["diff", "--name-only", before.trim(), after.trim()])?;
    let n = git(
        path,
        &[
            "rev-list",
            "--count",
            &format!("{}..{}", before.trim(), after.trim()),
        ],
    )?;
    let mut msg = format!("Pulled {} commits", n.trim());
    if dirty {
        msg.push_str(", your changes are back");
    }
    if lockfiles_changed(&changed) {
        msg.push_str("; dependencies changed, install them next");
    }
    Ok(Outcome {
        message: msg,
        path: Some(format!("{RESET_TO}{}\n{}", path.display(), before.trim())),
    })
}

/// The repo's main branch, from the remote's HEAD; "main" when unknown.
pub fn main_branch(path: &Path) -> String {
    git(
        path,
        &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
    )
    .map(|s| s.trim().trim_start_matches("origin/").to_owned())
    .unwrap_or_else(|_| "main".into())
}

/// Marks a result whose merge stopped on conflicts: the island opens them.
pub const MERGE_AT: &str = "git-merge:";
/// The stash message for changes put aside while the branch updates.
const UPDATE_STASH: &str = "sidekick: update";

fn dirty(path: &Path) -> Result<bool, ActionError> {
    Ok(
        !git(path, &["status", "--porcelain", "--untracked-files=no"])?
            .trim()
            .is_empty(),
    )
}

/// Puts back the changes put aside for the update, when they are on top.
fn unstash(path: &Path) -> Result<bool, ActionError> {
    let top = git(path, &["stash", "list", "-n", "1", "--format=%s"]).unwrap_or_default();
    if !top.contains(UPDATE_STASH) {
        return Ok(false);
    }
    git(path, &["stash", "pop", "--quiet"]).map_err(|e| {
        ActionError::Failed(format!(
            "Your changes clash with the update; they are kept in git stash ({e})"
        ))
    })?;
    Ok(true)
}

/// The commits on the main branch that your branch does not have yet.
pub fn branch_commits(path: &Path) -> Result<Outcome, ActionError> {
    repo(path)?;
    let main = main_branch(path);
    let range = format!("HEAD..origin/{main}");
    let log = git(
        path,
        &["log", "--format=%an\x1f%s\x1f%cr", "-n", "10", &range],
    )?;
    let lines: Vec<String> = log
        .lines()
        .filter_map(|l| {
            let mut p = l.split('\x1f');
            Some(format!("{} · {} · {}", p.next()?, p.next()?, p.next()?))
        })
        .collect();
    if lines.is_empty() {
        return Ok(Outcome::msg(format!("Nothing new on {main}")));
    }
    Ok(Outcome::msg(lines.join("\n")))
}

/// Brings the main branch's new commits into the feature branch you are on,
/// with a merge, never a rebase, so a branch others share keeps working.
/// Uncommitted changes are put aside first and back after. On a conflict
/// the merge stays open so the island can walk through each file.
pub fn update_branch(path: &Path) -> Result<Outcome, ActionError> {
    repo(path)?;
    let main = main_branch(path);
    let branch = git(path, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    if branch.trim() == main {
        return Err(ActionError::Failed(
            "You are on the main branch; Pull updates it".into(),
        ));
    }
    let stashed = dirty(path)?;
    if stashed {
        git(path, &["stash", "push", "--quiet", "-m", UPDATE_STASH])?;
    }
    let _ = git(path, &["fetch", "--quiet", "--no-tags", "origin", &main]);
    let before = git(path, &["rev-parse", "HEAD"])?;
    let base = format!("origin/{main}");
    if let Err(e) = git(path, &["merge", "--no-edit", "--quiet", &base]) {
        let open = conflicted(path);
        if !open.is_empty() {
            return Ok(Outcome {
                message: format!(
                    "{} {} you",
                    open.len(),
                    if open.len() == 1 {
                        "file needs"
                    } else {
                        "files need"
                    }
                ),
                path: Some(format!("{MERGE_AT}{}", path.display())),
            });
        }
        let _ = git(path, &["merge", "--abort"]);
        if stashed {
            unstash(path)?;
        }
        return Err(ActionError::Failed(format!(
            "Could not merge {main}, so nothing changed ({e})"
        )));
    }
    let back = if stashed { unstash(path)? } else { false };
    let after = git(path, &["rev-parse", "HEAD"])?;
    if before.trim() == after.trim() {
        return Ok(Outcome::msg(format!("Already up to date with {main}")));
    }
    let mut message = format!("Merged {main} into {}", branch.trim());
    if back {
        message.push_str(", your changes are back");
    }
    Ok(Outcome {
        message,
        path: Some(format!("{RESET_TO}{}\n{}", path.display(), before.trim())),
    })
}

/// Files the open merge left with conflicts.
fn conflicted(path: &Path) -> Vec<String> {
    git(path, &["diff", "--name-only", "--diff-filter=U"])
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .filter(|l| !l.is_empty())
        .collect()
}

/// One file that clashes: how many spots, and both sides of the first.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct Clash {
    pub file: String,
    pub spots: usize,
    pub mine: String,
    pub theirs: String,
}

/// The merge in progress: which branch, from where, and what still clashes.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MergeState {
    pub branch: String,
    pub main: String,
    pub stashed: bool,
    pub files: Vec<Clash>,
}

/// Splits conflict markers into the spot count and both sides of the first.
pub fn clash_of(text: &str) -> (usize, String, String) {
    let (mut spots, mut mine, mut theirs) = (0, Vec::new(), Vec::new());
    let mut side = 0; // 0 outside, 1 mine, 2 base, 3 theirs
    for line in text.lines() {
        if line.starts_with("<<<<<<< ") || line == "<<<<<<<" {
            spots += 1;
            side = 1;
        } else if side > 0 && line.starts_with("|||||||") {
            side = 2;
        } else if side > 0 && line == "=======" {
            side = 3;
        } else if side > 0 && line.starts_with(">>>>>>>") {
            side = 0;
        } else if spots == 1 && side == 1 && mine.len() < 12 {
            mine.push(line);
        } else if spots == 1 && side == 3 && theirs.len() < 12 {
            theirs.push(line);
        }
    }
    (spots, mine.join("\n"), theirs.join("\n"))
}

/// Keeps both sides of every conflict, yours first.
pub fn keep_both(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut skip = false;
    for line in text.split_inclusive('\n') {
        let bare = line.trim_end_matches(['\r', '\n']);
        if bare.starts_with("<<<<<<<") || bare.starts_with(">>>>>>>") || bare == "=======" {
            skip = false;
            continue;
        }
        if bare.starts_with("|||||||") {
            skip = true;
            continue;
        }
        if !skip {
            out.push_str(line);
        }
    }
    out
}

pub fn merge_state(path: &Path) -> Result<MergeState, ActionError> {
    repo(path)?;
    let top = git(path, &["stash", "list", "-n", "1", "--format=%s"]).unwrap_or_default();
    let files = conflicted(path)
        .into_iter()
        .map(|file| {
            let text = std::fs::read_to_string(path.join(&file)).unwrap_or_default();
            let (spots, mine, theirs) = clash_of(&text);
            Clash {
                file,
                spots: spots.max(1),
                mine,
                theirs,
            }
        })
        .collect();
    Ok(MergeState {
        branch: git(path, &["rev-parse", "--abbrev-ref", "HEAD"])?
            .trim()
            .to_owned(),
        main: main_branch(path),
        stashed: top.contains(UPDATE_STASH),
        files,
    })
}

/// Solves one file: "mine", "theirs" or "both".
pub fn merge_keep(path: &Path, file: &str, side: &str) -> Result<(), ActionError> {
    repo(path)?;
    if !conflicted(path).iter().any(|f| f == file) {
        return Err(ActionError::Invalid(format!("{file} has no conflict")));
    }
    match side {
        "mine" => {
            git(path, &["checkout", "--ours", "--", file])?;
        }
        "theirs" => {
            git(path, &["checkout", "--theirs", "--", file])?;
        }
        "both" => {
            let at = path.join(file);
            let text = std::fs::read_to_string(&at).map_err(crate::fail)?;
            std::fs::write(&at, keep_both(&text)).map_err(crate::fail)?;
        }
        _ => return Err(ActionError::Invalid(format!("Unknown side {side}"))),
    }
    git(path, &["add", "--", file])?;
    Ok(())
}

/// Commits the merge once nothing clashes, and puts your changes back.
pub fn merge_finish(path: &Path) -> Result<Outcome, ActionError> {
    repo(path)?;
    let left = conflicted(path).len();
    if left > 0 {
        return Err(ActionError::Failed(format!("{left} files still need you")));
    }
    let before = git(path, &["rev-parse", "HEAD"])?;
    git(path, &["commit", "--no-edit", "--quiet"])?;
    let back = unstash(path)?;
    let main = main_branch(path);
    Ok(Outcome {
        message: if back {
            format!("Merged {main}, your changes are back")
        } else {
            format!("Merged {main}")
        },
        path: Some(format!("{RESET_TO}{}\n{}", path.display(), before.trim())),
    })
}

/// Backs out of the merge and puts your changes back: as before the update.
pub fn merge_undo(path: &Path) -> Result<Outcome, ActionError> {
    repo(path)?;
    git(path, &["merge", "--abort"])?;
    unstash(path)?;
    Ok(Outcome::msg("Back exactly as before the update"))
}

/// After your PR merged: back to the main branch, pull it, and delete the
/// merged branch when git agrees it is merged.
pub fn after_merge(path: &Path, branch: &str) -> Result<Outcome, ActionError> {
    repo(path)?;
    let main = main_branch(path);
    if branch.is_empty() || branch == main {
        return Err(ActionError::Failed("That is the main branch".into()));
    }
    let current = git(path, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    if current.trim() == branch {
        git(path, &["checkout", "--quiet", &main])?;
    }
    let _ = git(path, &["fetch", "--quiet", "--prune"]);
    let pulled = if git(path, &["rev-parse", "--abbrev-ref", "HEAD"])?.trim() == main {
        git(path, &["pull", "--ff-only", "--quiet"]).is_ok()
    } else {
        false
    };
    // -d refuses a branch git does not see as merged (a squash merge): then
    // it is only deleted when its remote branch is gone too.
    let deleted = git(path, &["branch", "-d", branch]).is_ok()
        || (git(
            path,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("origin/{branch}"),
            ],
        )
        .is_err()
            && git(path, &["branch", "-D", branch]).is_ok());
    let mut msg = format!("On {main}");
    if pulled {
        msg.push_str(", pulled");
    }
    msg.push_str(if deleted {
        ", deleted the branch"
    } else {
        ", kept the branch (it has commits that are not merged)"
    });
    Ok(Outcome::msg(msg))
}

/// The folder a clone of `url` goes in: the last part, without `.git`.
pub fn clone_name(url: &str) -> Option<String> {
    let last = url
        .trim()
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .rsplit(['/', ':'])
        .next()?;
    let ok = !last.is_empty()
        && last != ".."
        && last
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    ok.then(|| last.to_owned())
}

/// Clones `url` into a new folder inside `dir`.
pub fn clone(url: &str, dir: &Path) -> Result<Outcome, ActionError> {
    let name =
        clone_name(url).ok_or_else(|| ActionError::Failed("That is not a repo link".into()))?;
    if !(url.starts_with("https://") || url.starts_with("git@") || url.starts_with("ssh://")) {
        return Err(ActionError::Failed("That is not a repo link".into()));
    }
    std::fs::create_dir_all(dir).map_err(|e| ActionError::Failed(e.to_string()))?;
    let target = dir.join(&name);
    if target.exists() {
        return Err(ActionError::Failed(format!(
            "{} already exists",
            target.display()
        )));
    }
    let mut cmd = Command::new("git");
    cmd.arg("clone")
        .arg("--quiet")
        .arg("--")
        .arg(url)
        .arg(&target)
        .env("GIT_TERMINAL_PROMPT", "0");
    hidden(&mut cmd);
    let out = cmd
        .output()
        .map_err(|e| ActionError::Failed(e.to_string()))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(ActionError::Failed(
            err.lines()
                .last()
                .unwrap_or("clone failed")
                .trim()
                .to_owned(),
        ));
    }
    Ok(Outcome {
        message: format!("Cloned {name} into {}", dir.display()),
        path: Some(target.to_string_lossy().into_owned()),
    })
}

/// Which package manager a project uses, by its lockfile.
pub fn manager(path: &Path) -> Option<(&'static str, &'static [&'static str])> {
    MANAGERS
        .iter()
        .find(|(lock, _, _)| path.join(lock).exists())
        .map(|(_, tool, args)| (*tool, *args))
}

const INSTALL_TIMEOUT: Duration = Duration::from_secs(15 * 60);

pub fn install(path: &Path) -> Result<Outcome, ActionError> {
    let (tool, args) =
        manager(path).ok_or_else(|| ActionError::Failed("no lockfile found".into()))?;
    let exe =
        which::which(tool).map_err(|_| ActionError::Failed(format!("{tool} is not installed")))?;
    let mut cmd = Command::new(exe);
    cmd.args(args)
        .current_dir(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    hidden(&mut cmd);
    let mut child = cmd.spawn().map_err(fail)?;
    let start = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(fail)? {
            if status.success() {
                return Ok(Outcome::msg(format!("{tool} {} finished", args.join(" "))));
            }
            let mut err = String::new();
            if let Some(mut e) = child.stderr.take() {
                let _ = std::io::Read::read_to_string(&mut e, &mut err);
            }
            return Err(ActionError::Failed(format!(
                "{tool} failed: {}",
                err.lines()
                    .rev()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or("see the terminal")
                    .trim()
            )));
        }
        if start.elapsed() > INSTALL_TIMEOUT {
            let _ = child.kill();
            return Err(ActionError::Failed(format!("{tool} took too long")));
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

/// Copies `.env.example` to `.env`; never overwrites.
pub fn create_env(path: &Path) -> Result<Outcome, ActionError> {
    let example = path.join(".env.example");
    let env = path.join(".env");
    if env.exists() {
        return Err(ActionError::Failed(".env already exists".into()));
    }
    std::fs::copy(&example, &env).map_err(fail)?;
    Ok(Outcome {
        message: "Created .env from .env.example; fill in the values".into(),
        path: Some(env.to_string_lossy().into_owned()),
    })
}

fn docker_desktop() -> Option<PathBuf> {
    let base = std::env::var_os("ProgramFiles").map(PathBuf::from)?;
    let exe = base
        .join("Docker")
        .join("Docker")
        .join("Docker Desktop.exe");
    exe.is_file().then_some(exe)
}

pub fn start_docker() -> Result<Outcome, ActionError> {
    let exe = docker_desktop()
        .ok_or_else(|| ActionError::Failed("Docker Desktop is not installed".into()))?;
    system::spawn_detached(Command::new(exe))?;
    Ok(Outcome::msg("Starting Docker Desktop"))
}

/// Editor plus a terminal in the project folder.
pub fn launch(path: &Path, editor: Option<(&Path, &str)>) -> Result<Outcome, ActionError> {
    let mut opened = Vec::new();
    if let Some((code, name)) = editor {
        let mut cmd = Command::new(code);
        cmd.arg(path);
        system::spawn_detached(cmd)?;
        opened.push(name);
    }
    if let Ok(wt) = which::which("wt") {
        let mut cmd = Command::new(wt);
        cmd.arg("-d").arg(path);
        // Windows Terminal must be visible, so no hidden flag here.
        cmd.spawn().map(drop).map_err(fail)?;
        opened.push("a terminal");
    }
    if opened.is_empty() {
        open::that_detached(path).map_err(fail)?;
        opened.push("the folder");
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(Outcome::msg(format!(
        "Opened {name} in {}",
        opened.join(" and ")
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spots_lockfile_changes_and_managers() {
        assert!(lockfiles_changed("src/a.ts\npnpm-lock.yaml\n"));
        assert!(lockfiles_changed("api/uv.lock"));
        assert!(!lockfiles_changed("README.md\npackage.json"));
        let dir = std::env::temp_dir().join(format!("sidekick-dev-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(manager(&dir).is_none());
        std::fs::write(dir.join("uv.lock"), "").unwrap();
        assert_eq!(manager(&dir).unwrap().0, "uv");
        std::fs::write(dir.join(".env.example"), "KEY=").unwrap();
        create_env(&dir).unwrap();
        assert!(dir.join(".env").exists());
        assert!(create_env(&dir).is_err(), "never overwrites");
        assert!(pull(&dir, false).is_err(), "not a repo");
        assert_eq!(
            clone_name("https://github.com/a/shop.git").as_deref(),
            Some("shop")
        );
        assert_eq!(clone_name("git@gitlab.com:t/app").as_deref(), Some("app"));
        assert!(clone_name("https://github.com/a/..").is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn reads_and_keeps_both_sides() {
        let t = "a\n<<<<<<< HEAD\nmine\n||||||| base\nold\n=======\ntheirs\n>>>>>>> main\nz\n<<<<<<< HEAD\nx\n=======\ny\n>>>>>>> main\n";
        let (n, mine, theirs) = clash_of(t);
        assert_eq!((n, mine.as_str(), theirs.as_str()), (2, "mine", "theirs"));
        assert_eq!(keep_both(t), "a\nmine\ntheirs\nz\nx\ny\n");
    }

    #[test]
    fn updates_a_feature_branch_from_main() {
        let base = std::env::temp_dir().join(format!("sk-ub-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let (origin, a, b) = (base.join("o.git"), base.join("a"), base.join("b"));
        let run = |dir: &Path, args: &[&str]| {
            assert!(
                Command::new("git")
                    .arg("-C")
                    .arg(dir)
                    .args(args)
                    .output()
                    .unwrap()
                    .status
                    .success(),
                "{args:?}"
            );
        };
        run(
            &base,
            &[
                "init",
                "-q",
                "--bare",
                "-b",
                "main",
                origin.to_str().unwrap(),
            ],
        );
        for d in [&a, &b] {
            run(
                &base,
                &["clone", "-q", origin.to_str().unwrap(), d.to_str().unwrap()],
            );
            run(d, &["config", "user.email", "t@t"]);
            run(d, &["config", "user.name", "T"]);
        }
        std::fs::write(a.join("f"), "1").unwrap();
        run(&a, &["add", "."]);
        run(&a, &["commit", "-qm", "one"]);
        run(&a, &["push", "-q", "origin", "HEAD:main"]);
        run(&b, &["pull", "-q", "origin", "main"]);
        run(&b, &["remote", "set-head", "origin", "main"]);
        run(&b, &["checkout", "-q", "-b", "feature"]);
        std::fs::write(b.join("g"), "mine").unwrap();
        run(&b, &["add", "."]);
        run(&b, &["commit", "-qm", "mine"]);
        std::fs::write(a.join("f"), "2").unwrap();
        run(&a, &["commit", "-qam", "two"]);
        run(&a, &["push", "-q", "origin", "HEAD:main"]);
        let out = update_branch(&b).unwrap();
        assert_eq!(out.message, "Merged main into feature");
        assert_eq!(std::fs::read_to_string(b.join("f")).unwrap(), "2");
        assert!(out.path.unwrap().starts_with(RESET_TO));
        // A clash keeps the merge open, with your uncommitted change put aside.
        std::fs::write(b.join("f"), "b").unwrap();
        run(&b, &["commit", "-qam", "b"]);
        std::fs::write(a.join("f"), "a").unwrap();
        run(&a, &["commit", "-qam", "a"]);
        run(&a, &["push", "-q", "origin", "HEAD:main"]);
        std::fs::write(b.join("g"), "wip").unwrap();
        let head = git(&b, &["rev-parse", "HEAD"]).unwrap();
        let out = update_branch(&b).unwrap();
        assert_eq!(out.message, "1 file needs you");
        assert!(out.path.unwrap().starts_with(MERGE_AT));
        let state = merge_state(&b).unwrap();
        assert!(state.stashed);
        assert_eq!(state.files.len(), 1);
        assert_eq!(
            (state.files[0].mine.as_str(), state.files[0].theirs.as_str()),
            ("b", "a")
        );
        assert!(branch_commits(&b).unwrap().message.contains("T · a"));
        // Undo puts everything back as it was.
        merge_undo(&b).unwrap();
        assert_eq!(git(&b, &["rev-parse", "HEAD"]).unwrap(), head);
        assert_eq!(std::fs::read_to_string(b.join("g")).unwrap(), "wip");
        // Again, then solve it and finish.
        update_branch(&b).unwrap();
        assert!(merge_finish(&b).is_err(), "still clashes");
        assert!(merge_keep(&b, "g", "mine").is_err(), "only clashing files");
        merge_keep(&b, "f", "theirs").unwrap();
        let done = merge_finish(&b).unwrap();
        assert_eq!(done.message, "Merged main, your changes are back");
        assert_eq!(std::fs::read_to_string(b.join("f")).unwrap(), "a");
        assert_eq!(std::fs::read_to_string(b.join("g")).unwrap(), "wip");
        let _ = std::fs::remove_dir_all(&base);
    }
}
