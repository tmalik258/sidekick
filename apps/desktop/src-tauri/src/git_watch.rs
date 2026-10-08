//! Git that keeps up: new commits show before your editor notices them.
//! The repo you are working in is fetched when you switch to it (at most
//! every 30 seconds) and every minute while you work in it; the rest every
//! five minutes. Waking from sleep and getting back online fetch right
//! away. With a GitHub sign-in, a cheap conditional request every 15
//! seconds says when the remote moved; "nothing changed" answers are free.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use sidekick_core::Event;
use sidekick_sensors::ReposSensor;
use tauri::{AppHandle, Manager};

use crate::state::{AppState, lock};

pub const INCOMING: &str = "dev.incoming";
pub const BASE_MOVED: &str = "dev.base_moved";

const SWITCH_MIN: Duration = Duration::from_secs(30);
const ACTIVE_EVERY: Duration = Duration::from_secs(60);
const REST_EVERY: Duration = Duration::from_secs(5 * 60);
const TICK: Duration = Duration::from_secs(15);
/// A tick this late means the PC slept.
const SLEPT: Duration = Duration::from_secs(90);
const FETCH_TIMEOUT: Duration = Duration::from_secs(20);
/// Background repos fetched per tick, so a big folder never stalls.
const PER_TICK: usize = 3;

const TERMINALS: &[&str] = &[
    "windowsterminal.exe",
    "wt.exe",
    "pwsh.exe",
    "powershell.exe",
    "cmd.exe",
    "wezterm-gui.exe",
    "alacritty.exe",
];

#[derive(Default)]
struct Watch {
    active: Option<PathBuf>,
    fetched: HashMap<PathBuf, Instant>,
    /// The upstream commit last told about, per repo.
    told: HashMap<PathBuf, String>,
    /// GitHub's ETag per repo and branch.
    etags: HashMap<String, String>,
}

static WATCH: Mutex<Option<Watch>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut Watch) -> R) -> R {
    let mut g = WATCH.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(Watch::default))
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
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// The repo a window is about: an editor's project, or a terminal whose
/// title shows a repo's folder.
pub fn repo_for_window(exe: &str, title: &str, repos: &[PathBuf]) -> Option<PathBuf> {
    let exe = exe.to_ascii_lowercase();
    if TERMINALS.contains(&exe.as_str()) {
        let t = title.to_lowercase().replace('/', "\\");
        return repos
            .iter()
            .filter(|r| {
                let p = r.to_string_lossy().to_lowercase().replace('/', "\\");
                t.contains(&p)
            })
            .max_by_key(|r| r.as_os_str().len())
            .cloned();
    }
    let project = crate::timetrack::project_from_title(&exe, title);
    if project.is_empty() {
        return None;
    }
    repos
        .iter()
        .find(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(&project))
        })
        .cloned()
}

fn enabled(app: &AppHandle) -> bool {
    let state = app.state::<AppState>();
    let s = lock(&state.settings);
    !s.pause.is_active(chrono::Utc::now()) && s.sensor_enabled(ReposSensor::ID)
}

/// A window came to the front: an editor or terminal on a repo makes it the
/// active one and fetches it now, unless that happened in the last 30 s.
pub fn on_window(app: &AppHandle, payload: &Value) {
    if !enabled(app) {
        return;
    }
    let exe = payload["exe"].as_str().unwrap_or_default().to_owned();
    let title = payload["title"].as_str().unwrap_or_default().to_owned();
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let repos = crate::projects::list(&app);
        let Some(repo) = repo_for_window(&exe, &title, &repos) else {
            return;
        };
        let due = with(|w| {
            w.active = Some(repo.clone());
            w.fetched
                .get(&repo)
                .is_none_or(|t| t.elapsed() >= SWITCH_MIN)
        });
        if due {
            fetch_and_tell(&app, &repo);
        }
    });
}

/// What is new upstream, for the card.
pub fn incoming(repo: &Path) -> Option<Value> {
    let upstream = git(repo, &["rev-parse", "@{u}"])?;
    let behind: u64 = git(repo, &["rev-list", "--count", "HEAD..@{u}"])?
        .parse()
        .ok()?;
    if behind == 0 {
        return None;
    }
    let ahead: u64 = git(repo, &["rev-list", "--count", "@{u}..HEAD"])
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let log = git(
        repo,
        &["log", "--format=%an\x1f%s", "-n", "5", "HEAD..@{u}"],
    )
    .unwrap_or_default();
    let commits: Vec<String> = log
        .lines()
        .filter_map(|l| l.split_once('\x1f'))
        .map(|(who, what)| format!("{who}: {what}"))
        .collect();
    let dirty = git(repo, &["status", "--porcelain", "--untracked-files=no"])
        .is_some_and(|s| !s.is_empty());
    let branch = git(repo, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default();
    Some(json!({
        "name": repo.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        "path": repo.to_string_lossy(),
        "branch": branch,
        "behind": behind,
        "upstream": upstream,
        "commits": commits.join("\n"),
        "dirty": dirty,
        "diverged": ahead > 0,
    }))
}

/// What the main branch gained while you work on a feature branch.
pub fn base_moved(repo: &Path) -> Option<Value> {
    let base = sidekick_actions::dev::main_branch(repo);
    let branch = git(repo, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    if branch == base || branch == "HEAD" {
        return None;
    }
    let remote = format!("origin/{base}");
    let upstream = git(repo, &["rev-parse", &remote])?;
    let range = format!("HEAD..{remote}");
    let behind: u64 = git(repo, &["rev-list", "--count", &range])?.parse().ok()?;
    if behind == 0 {
        return None;
    }
    let log = git(repo, &["log", "--format=%an\x1f%s", "-n", "5", &range]).unwrap_or_default();
    let commits: Vec<String> = log
        .lines()
        .filter_map(|l| l.split_once('\x1f'))
        .map(|(who, what)| format!("{who}: {what}"))
        .collect();
    let dirty = git(repo, &["status", "--porcelain", "--untracked-files=no"])
        .is_some_and(|s| !s.is_empty());
    Some(json!({
        "name": repo.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        "path": repo.to_string_lossy(),
        "branch": branch,
        "base": base,
        "behind": behind,
        "upstream": upstream,
        "commits": commits.join("\n"),
        "dirty": dirty,
    }))
}

/// Fetches one repo and says so when it has new commits not told about yet.
fn fetch_and_tell(app: &AppHandle, repo: &Path) {
    sidekick_sensors::repos::fetch(repo, FETCH_TIMEOUT);
    with(|w| w.fetched.insert(repo.to_owned(), Instant::now()));
    if let Some(payload) = base_moved(repo) {
        // Told once per new main commit, under its own key.
        let key = repo.join(".sidekick-base");
        let upstream = payload["upstream"].as_str().unwrap_or_default().to_owned();
        let new =
            with(|w| w.told.insert(key, upstream.clone()).as_deref() != Some(upstream.as_str()));
        if new {
            app.state::<AppState>()
                .bus
                .publish(Event::new(BASE_MOVED, ReposSensor::ID, payload));
        }
    }
    let Some(payload) = incoming(repo) else {
        return;
    };
    let upstream = payload["upstream"].as_str().unwrap_or_default().to_owned();
    let new = with(|w| {
        w.told.insert(repo.to_owned(), upstream.clone()).as_deref() != Some(upstream.as_str())
    });
    if new {
        app.state::<AppState>()
            .bus
            .publish(Event::new(INCOMING, ReposSensor::ID, payload));
    }
}

/// The GitHub sign-in of the `gh` CLI, when there is one.
fn gh_token() -> Option<String> {
    static TOKEN: Mutex<Option<(Instant, Option<String>)>> = Mutex::new(None);
    let mut g = TOKEN.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, t)) = g.as_ref()
        && at.elapsed() < Duration::from_secs(10 * 60)
    {
        return t.clone();
    }
    let t = which::which("gh").ok().and_then(|gh| {
        let mut cmd = Command::new(gh);
        cmd.args(["auth", "token"]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        let out = cmd.output().ok()?;
        let t = String::from_utf8_lossy(&out.stdout).trim().to_owned();
        (out.status.success() && !t.is_empty()).then_some(t)
    });
    *g = Some((Instant::now(), t.clone()));
    t
}

/// Asks GitHub whether the active repo's branch moved. A "not modified"
/// answer does not count against the rate limit.
async fn github_moved(http: &reqwest::Client, repo: &Path, token: &str) -> bool {
    let Some(url) = sidekick_sensors::repos::github_url(repo) else {
        return false;
    };
    let slug = url.trim_start_matches("https://github.com/").to_owned();
    let (repo2, r3) = (repo.to_owned(), repo.to_owned());
    let branch = tauri::async_runtime::spawn_blocking(move || {
        git(&repo2, &["rev-parse", "--abbrev-ref", "HEAD"])
    })
    .await
    .ok()
    .flatten()
    .unwrap_or_default();
    if branch.is_empty() || branch == "HEAD" {
        return false;
    }
    let key = format!("{slug}@{branch}");
    let mut req = http
        .get(format!(
            "https://api.github.com/repos/{slug}/commits/{branch}"
        ))
        .header("Accept", "application/vnd.github.sha")
        .header("User-Agent", "Sidekick")
        .bearer_auth(token);
    if let Some(tag) = with(|w| w.etags.get(&key).cloned()) {
        req = req.header("If-None-Match", tag);
    }
    let Ok(res) = req.send().await else {
        return false;
    };
    if res.status() == reqwest::StatusCode::NOT_MODIFIED || !res.status().is_success() {
        return false;
    }
    if let Some(tag) = res.headers().get("etag").and_then(|v| v.to_str().ok()) {
        with(|w| w.etags.insert(key, tag.to_owned()));
    }
    let remote = res.text().await.unwrap_or_default();
    let local = tauri::async_runtime::spawn_blocking(move || git(&r3, &["rev-parse", "@{u}"]))
        .await
        .ok()
        .flatten()
        .unwrap_or_default();
    !remote.trim().is_empty() && remote.trim() != local
}

/// Repos that are due a background fetch, oldest first.
fn due_rest(repos: &[PathBuf], active: Option<&PathBuf>) -> Vec<PathBuf> {
    let mut due: Vec<(PathBuf, Option<Instant>)> = with(|w| {
        repos
            .iter()
            .filter(|r| Some(*r) != active)
            .map(|r| (r.clone(), w.fetched.get(r).copied()))
            .filter(|(_, t)| t.is_none_or(|t| t.elapsed() >= REST_EVERY))
            .collect()
    });
    due.sort_by_key(|(_, t)| *t);
    due.into_iter().take(PER_TICK).map(|(r, _)| r).collect()
}

pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_default();
        let mut last = Instant::now();
        let mut was_online = crate::net::online();
        loop {
            tokio::time::sleep(TICK).await;
            let woke = last.elapsed() > SLEPT;
            last = Instant::now();
            let online = crate::net::online();
            let back = online && !was_online;
            was_online = online;
            if !online || !enabled(&app) {
                continue;
            }
            let active = with(|w| w.active.clone());
            let mut now: Vec<PathBuf> = Vec::new();
            if let Some(a) = &active {
                let stale = with(|w| w.fetched.get(a).is_none_or(|t| t.elapsed() >= ACTIVE_EVERY));
                let moved = match tauri::async_runtime::spawn_blocking(gh_token)
                    .await
                    .ok()
                    .flatten()
                {
                    Some(token) if !stale && !woke && !back => github_moved(&http, a, &token).await,
                    _ => false,
                };
                if stale || moved || woke || back {
                    now.push(a.clone());
                }
            }
            let app2 = app.clone();
            let rest = tauri::async_runtime::spawn_blocking(move || {
                let repos = crate::projects::list(&app2);
                due_rest(&repos, active.as_ref())
            })
            .await
            .unwrap_or_default();
            now.extend(rest);
            let app2 = app.clone();
            let _ = tauri::async_runtime::spawn_blocking(move || {
                for r in now {
                    fetch_and_tell(&app2, &r);
                }
            })
            .await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(dir: &Path, args: &[&str]) {
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(args)
                .output()
                .unwrap()
                .status
                .success()
        );
    }

    #[test]
    fn finds_the_repo_a_window_is_on() {
        let repos = vec![
            PathBuf::from("C:\\code\\shop"),
            PathBuf::from("C:\\code\\shop\\api"),
        ];
        assert_eq!(
            repo_for_window("WindowsTerminal.exe", "pwsh in C:\\code\\shop\\api", &repos),
            Some(PathBuf::from("C:\\code\\shop\\api"))
        );
        assert_eq!(
            repo_for_window("notepad.exe", "shop - Notepad", &repos),
            None
        );
        // file_name() splits on this platform's separator.
        let here = vec![Path::new("code").join("shop")];
        assert_eq!(
            repo_for_window("cursor.exe", "main.rs - shop - Cursor", &here),
            Some(Path::new("code").join("shop"))
        );
    }

    #[test]
    fn says_what_is_new_upstream() {
        let base = std::env::temp_dir().join(format!("sk-gw-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (origin, a, b) = (base.join("origin.git"), base.join("a"), base.join("b"));
        std::fs::create_dir_all(&base).unwrap();
        run(
            &base,
            &["init", "--bare", "-b", "main", origin.to_str().unwrap()],
        );
        run(
            &base,
            &["clone", "-q", origin.to_str().unwrap(), a.to_str().unwrap()],
        );
        run(&a, &["config", "user.email", "t@t"]);
        run(&a, &["config", "user.name", "Ana"]);
        std::fs::write(a.join("f"), "1").unwrap();
        run(&a, &["add", "."]);
        run(&a, &["commit", "-qm", "first"]);
        run(&a, &["push", "-q", "origin", "HEAD:main"]);
        run(
            &base,
            &["clone", "-q", origin.to_str().unwrap(), b.to_str().unwrap()],
        );
        assert!(incoming(&b).is_none());
        std::fs::write(a.join("f"), "2").unwrap();
        run(&a, &["commit", "-qam", "fix login"]);
        run(&a, &["push", "-q", "origin", "HEAD:main"]);
        sidekick_sensors::repos::fetch(&b, FETCH_TIMEOUT);
        let v = incoming(&b).unwrap();
        assert_eq!(v["behind"], 1);
        assert_eq!(v["commits"], "Ana: fix login");
        assert_eq!(v["diverged"], false);
        // On a feature branch, main moving is told apart.
        run(&b, &["pull", "-q", "--ff-only"]);
        run(&b, &["checkout", "-q", "-b", "feature"]);
        run(&b, &["remote", "set-head", "origin", "main"]);
        assert!(base_moved(&b).is_none());
        std::fs::write(a.join("f"), "3").unwrap();
        run(&a, &["commit", "-qam", "tidy"]);
        run(&a, &["push", "-q", "origin", "HEAD:main"]);
        sidekick_sensors::repos::fetch(&b, FETCH_TIMEOUT);
        let m = base_moved(&b).unwrap();
        assert_eq!(m["base"], "main");
        assert_eq!(m["behind"], 1);
        assert_eq!(m["commits"], "Ana: tidy");
        let _ = std::fs::remove_dir_all(&base);
    }
}
