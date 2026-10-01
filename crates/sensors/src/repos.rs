//! End-of-day repos (FR-DEV-09): once a day, in the evening, finds git repos
//! in your code folders with uncommitted or unpushed work, so nothing is
//! left only on this laptop overnight.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use chrono::{Local, NaiveDate, Timelike};
use sidekick_core::{Event, EventBus, Sensitivity};
use tokio::task::JoinHandle;

use crate::{Sensor, SensorGate};

pub struct ReposSensor {
    pub roots: Vec<PathBuf>,
    /// Local hour (0 to 23) after which the check runs.
    pub hour: u32,
}

impl ReposSensor {
    pub const ID: &'static str = "repos";
    pub const EVENT_KIND: &'static str = "dev.unsaved_work";

    /// Common code folders under the home folder that exist on this PC.
    pub fn default_roots() -> Vec<PathBuf> {
        let Some(home) = dirs::home_dir() else {
            return Vec::new();
        };
        [
            "code",
            "projects",
            "Projects",
            "dev",
            "source/repos",
            "Documents/GitHub",
            "repos",
            "src",
        ]
        .iter()
        .map(|p| home.join(p))
        .filter(|p| p.is_dir())
        .collect()
    }
}

const CHECK_EVERY: Duration = Duration::from_secs(5 * 60);
const MAX_REPOS: usize = 200;

/// Git repos directly in `root` or one level down.
pub fn find_repos(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir()
            || path
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with('.'))
        {
            continue;
        }
        if path.join(".git").exists() {
            out.push(path);
        } else if let Ok(inner) = std::fs::read_dir(&path) {
            out.extend(
                inner
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.is_dir() && p.join(".git").exists()),
            );
        }
        if out.len() >= MAX_REPOS {
            break;
        }
    }
    out
}

fn git(repo: &Path, args: &[&str]) -> Option<String> {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(repo).args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let out = cmd.output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Runs `git fetch` quietly, giving up after `timeout` (no prompts: a repo
/// that needs a password just stays stale).
fn fetch(repo: &Path, timeout: Duration) {
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(repo)
        .args(["fetch", "--quiet", "--no-tags"])
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let Ok(mut child) = cmd.spawn() else { return };
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if let Ok(Some(_)) = child.try_wait() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// What a developer wants to know when a project comes to the front
/// (FR-DEV-04, 05, 07): branch, how far behind and ahead of its upstream,
/// uncommitted files, a missing `.env`, and whether it needs Docker.
pub fn status(repo: &Path) -> serde_json::Value {
    fetch(repo, Duration::from_secs(10));
    let count = |args: &[&str]| -> u64 {
        git(repo, args)
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0)
    };
    let branch = git(repo, &["rev-parse", "--abbrev-ref", "HEAD"])
        .map(|s| s.trim().to_owned())
        .unwrap_or_default();
    let behind = count(&["rev-list", "--count", "HEAD..@{u}"]);
    let ahead = count(&["rev-list", "--count", "@{u}..HEAD"]);
    let changed = git(repo, &["status", "--porcelain"]).map_or(0, |s| s.lines().count());
    let env_missing = !repo.join(".env").exists() && repo.join(".env.example").exists();
    let docker_needed = [
        "docker-compose.yml",
        "docker-compose.yaml",
        "compose.yml",
        "compose.yaml",
    ]
    .iter()
    .any(|f| repo.join(f).exists());
    let name = repo
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    serde_json::json!({
        "name": name,
        "path": repo.to_string_lossy(),
        "branch": branch,
        "behind": behind,
        "ahead": ahead,
        "changed": changed,
        "env_missing": env_missing,
        "docker_needed": docker_needed,
    })
}

/// The GitHub page of a repo's `origin`, if it is on github.com.
pub fn github_url(repo: &Path) -> Option<String> {
    let url = git(repo, &["remote", "get-url", "origin"])?;
    let url = url.trim().trim_end_matches(".git");
    let path = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("git@github.com:"))?;
    let valid = path.split('/').count() == 2
        && path
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '.'));
    valid.then(|| format!("https://github.com/{path}"))
}

/// Uncommitted file count and unpushed commit count, or None when the repo
/// is clean.
pub fn unsaved(repo: &Path) -> Option<(usize, usize)> {
    let changed = git(repo, &["status", "--porcelain"])?.lines().count();
    // No upstream (a local-only branch) counts as nothing unpushed.
    let ahead = git(repo, &["rev-list", "--count", "@{u}..HEAD"])
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);
    (changed > 0 || ahead > 0).then_some((changed, ahead))
}

impl Sensor for ReposSensor {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn spawn(self: Box<Self>, bus: EventBus, gate: SensorGate) -> JoinHandle<()> {
        tokio::spawn(async move {
            if self.roots.is_empty() {
                log::info!("repos sensor: no code folders found");
                return;
            }
            let mut last_day: Option<NaiveDate> = None;
            let mut tick = tokio::time::interval(CHECK_EVERY);
            loop {
                tick.tick().await;
                let now = Local::now();
                if now.hour() < self.hour || last_day == Some(now.date_naive()) {
                    continue;
                }
                if !gate.allows(Self::ID) {
                    continue;
                }
                last_day = Some(now.date_naive());
                let roots = self.roots.clone();
                let found = tokio::task::spawn_blocking(move || {
                    roots
                        .iter()
                        .flat_map(|r| find_repos(r))
                        .filter_map(|repo| unsaved(&repo).map(|u| (repo, u)))
                        .collect::<Vec<_>>()
                })
                .await
                .unwrap_or_default();
                if let Some(event) = unsaved_event(&found) {
                    bus.publish(event);
                }
            }
        })
    }
}

fn unsaved_event(found: &[(PathBuf, (usize, usize))]) -> Option<Event> {
    let (first, _) = found.first()?;
    let name = |p: &PathBuf| {
        p.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    let names: Vec<String> = found.iter().take(5).map(|(p, _)| name(p)).collect();
    let more = found.len().saturating_sub(names.len());
    Some(
        Event::new(
            ReposSensor::EVENT_KIND,
            ReposSensor::ID,
            serde_json::json!({
                "count": found.len(),
                "names": if more > 0 { format!("{} and {more} more", names.join(", ")) } else { names.join(", ") },
                "first": name(first),
                "first_path": first.display().to_string(),
                "changed": found.iter().map(|(_, (c, _))| c).sum::<usize>(),
                "unpushed": found.iter().map(|(_, (_, a))| a).sum::<usize>(),
            }),
        )
        .with_sensitivity(Sensitivity::Personal),
    )
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
            "{:?}",
            String::from_utf8_lossy(&ok.stderr)
        );
    }

    #[test]
    fn finds_repos_with_unsaved_work() {
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let root = std::env::temp_dir().join(format!("sidekick-repos-{}", std::process::id()));
        let clean = root.join("clean");
        let dirty = root.join("group").join("dirty");
        for repo in [&clean, &dirty] {
            std::fs::create_dir_all(repo).unwrap();
            run(repo, &["init", "-q"]);
            run(
                repo,
                &[
                    "-c",
                    "user.email=t@t",
                    "-c",
                    "user.name=t",
                    "commit",
                    "-q",
                    "--allow-empty",
                    "-m",
                    "init",
                ],
            );
        }
        std::fs::write(dirty.join("notes.md"), "wip").unwrap();

        let mut repos = find_repos(&root);
        repos.sort();
        assert_eq!(repos.len(), 2);
        assert_eq!(unsaved(&clean), None);
        assert_eq!(unsaved(&dirty), Some((1, 0)));

        let found: Vec<_> = repos
            .iter()
            .filter_map(|r| unsaved(r).map(|u| (r.clone(), u)))
            .collect();
        let e = unsaved_event(&found).unwrap();
        assert_eq!(e.payload["count"], 1);
        assert_eq!(e.payload["first"], "dirty");
        assert!(unsaved_event(&[]).is_none());

        // Status: a clone that is one commit behind, with a missing .env.
        let upstream = clean.clone();
        let clone = root.join("clone");
        run(
            &root,
            &[
                "clone",
                "-q",
                upstream.to_str().unwrap(),
                clone.to_str().unwrap(),
            ],
        );
        run(
            &upstream,
            &[
                "-c",
                "user.email=t@t",
                "-c",
                "user.name=t",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "next",
            ],
        );
        std::fs::write(clone.join(".env.example"), "KEY=").unwrap();
        std::fs::write(clone.join("compose.yaml"), "services: {}").unwrap();
        let st = status(&clone);
        assert_eq!(st["name"], "clone");
        assert_eq!(st["behind"], 1);
        assert_eq!(st["ahead"], 0);
        assert_eq!(st["env_missing"], true);
        assert_eq!(st["docker_needed"], true);
        assert_eq!(github_url(&clone), None, "a local remote is not GitHub");
        let _ = std::fs::remove_dir_all(root);
    }
}
