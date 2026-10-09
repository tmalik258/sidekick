//! GitHub connected properly: the `gh` CLI's sign-in when there is one (no
//! click), Composio otherwise. Agents gets a Repos list (branch, ahead and
//! behind, open PRs, CI, review requests); only what you can act on
//! reaches the island: CI failed on your PR, a review requested from you,
//! your PR merged.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{Value, json};
use sidekick_core::Event;
use sidekick_sensors::ReposSensor;
use tauri::{AppHandle, Manager};

use crate::state::{AppState, lock};

pub const NOTICE: &str = "dev.github";
const NOTICE_EVERY: Duration = Duration::from_secs(3 * 60);
const LIST_TTL: Duration = Duration::from_secs(60);
const MAX_REPOS: usize = 30;

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Pr {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub branch: String,
    pub mine: bool,
    /// "pass", "fail", "pending" or "none".
    pub ci: &'static str,
    /// The first failing check, with its link.
    pub failing: Option<(String, String)>,
    pub review_requested: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoRow {
    pub name: String,
    pub path: String,
    pub slug: Option<String>,
    pub branch: String,
    pub ahead: u64,
    pub behind: u64,
    pub changed: u64,
    pub prs: Vec<Pr>,
    /// CI on the current branch's PR, when it has one.
    pub ci: &'static str,
    pub reviews: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    /// "gh", "composio" or "none".
    pub via: &'static str,
    pub repos: Vec<RepoRow>,
}

fn hide(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

/// Runs `gh` and parses its JSON, or None when gh is missing or signed out.
fn gh(args: &[&str]) -> Option<Value> {
    let exe = which::which("gh").ok()?;
    let mut cmd = Command::new(exe);
    cmd.args(args).env("GH_PROMPT_DISABLED", "1");
    hide(&mut cmd);
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    serde_json::from_slice(&out.stdout).ok()
}

fn gh_signed_in() -> bool {
    static AT: Mutex<Option<(Instant, bool)>> = Mutex::new(None);
    let mut g = AT.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, ok)) = *g
        && at.elapsed() < Duration::from_secs(5 * 60)
    {
        return ok;
    }
    let ok = which::which("gh").is_ok_and(|exe| {
        let mut cmd = Command::new(exe);
        cmd.args(["auth", "status"]);
        hide(&mut cmd);
        cmd.output().is_ok_and(|o| o.status.success())
    });
    *g = Some((Instant::now(), ok));
    ok
}

fn git(repo: &Path, args: &[&str]) -> String {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(repo).args(args);
    hide(&mut cmd);
    cmd.output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default()
}

/// The overall state of a status rollup, and its first failing check.
pub fn rollup(checks: &[Value]) -> (&'static str, Option<(String, String)>) {
    if checks.is_empty() {
        return ("none", None);
    }
    let mut pending = false;
    for c in checks {
        // CheckRun has conclusion/status; StatusContext has state.
        let state = c["conclusion"]
            .as_str()
            .filter(|s| !s.is_empty())
            .or_else(|| c["state"].as_str())
            .unwrap_or("")
            .to_ascii_uppercase();
        match state.as_str() {
            "FAILURE" | "ERROR" | "TIMED_OUT" | "CANCELLED" | "ACTION_REQUIRED"
            | "STARTUP_FAILURE" => {
                let name = c["name"]
                    .as_str()
                    .or_else(|| c["context"].as_str())
                    .unwrap_or("A check");
                let link = c["detailsUrl"]
                    .as_str()
                    .or_else(|| c["targetUrl"].as_str())
                    .unwrap_or("");
                return ("fail", Some((name.to_owned(), link.to_owned())));
            }
            "SUCCESS" | "NEUTRAL" | "SKIPPED" => {}
            _ => pending = true,
        }
    }
    (if pending { "pending" } else { "pass" }, None)
}

fn me() -> Option<String> {
    static ME: Mutex<Option<String>> = Mutex::new(None);
    let mut g = ME.lock().unwrap_or_else(|e| e.into_inner());
    if g.is_none() {
        *g = gh(&["api", "user"]).and_then(|v| v["login"].as_str().map(String::from));
    }
    g.clone()
}

/// Open PRs of one repo through gh.
fn prs_gh(slug: &str, me: Option<&str>) -> Vec<Pr> {
    let Some(v) = gh(&[
        "pr",
        "list",
        "-R",
        slug,
        "--state",
        "open",
        "--limit",
        "20",
        "--json",
        "number,title,url,author,headRefName,statusCheckRollup,reviewRequests",
    ]) else {
        return Vec::new();
    };
    v.as_array()
        .into_iter()
        .flatten()
        .map(|p| {
            let checks = p["statusCheckRollup"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            let (ci, failing) = rollup(&checks);
            Pr {
                number: p["number"].as_u64().unwrap_or(0),
                title: p["title"].as_str().unwrap_or("").to_owned(),
                url: p["url"].as_str().unwrap_or("").to_owned(),
                branch: p["headRefName"].as_str().unwrap_or("").to_owned(),
                mine: me.is_some_and(|m| p["author"]["login"].as_str() == Some(m)),
                ci,
                failing,
                review_requested: me.is_some_and(|m| {
                    p["reviewRequests"]
                        .as_array()
                        .is_some_and(|r| r.iter().any(|x| x["login"].as_str() == Some(m)))
                }),
            }
        })
        .collect()
}

/// Open PRs of one repo through Composio's GitHub tools.
async fn prs_composio(app: &AppHandle, slug: &str) -> Vec<Pr> {
    let c = lock(&app.state::<AppState>().settings).composio.clone();
    let Some((owner, repo)) = slug.split_once('/') else {
        return Vec::new();
    };
    let Ok(v) = crate::composio::run_tool(
        &c,
        "GITHUB_LIST_PULL_REQUESTS",
        json!({ "owner": owner, "repo": repo, "state": "open", "per_page": 20 }),
    )
    .await
    else {
        return Vec::new();
    };
    let list = crate::composio_api::find_array(&v, &["pull_requests", "data", "items"])
        .cloned()
        .or_else(|| v.as_array().cloned())
        .unwrap_or_default();
    list.iter()
        .map(|p| Pr {
            number: p["number"].as_u64().unwrap_or(0),
            title: p["title"].as_str().unwrap_or("").to_owned(),
            url: p["html_url"].as_str().unwrap_or("").to_owned(),
            branch: p["head"]["ref"].as_str().unwrap_or("").to_owned(),
            ci: "none",
            ..Default::default()
        })
        .collect()
}

fn row(path: &Path, slug: Option<String>, prs: Vec<Pr>) -> RepoRow {
    let branch = git(path, &["rev-parse", "--abbrev-ref", "HEAD"]);
    let n = |args: &[&str]| git(path, args).parse().unwrap_or(0);
    let ci = prs
        .iter()
        .find(|p| p.branch == branch)
        .map_or("none", |p| p.ci);
    RepoRow {
        name: path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        path: path.to_string_lossy().into_owned(),
        slug,
        ahead: n(&["rev-list", "--count", "@{u}..HEAD"]),
        behind: n(&["rev-list", "--count", "HEAD..@{u}"]),
        changed: git(path, &["status", "--porcelain"]).lines().count() as u64,
        reviews: prs.iter().filter(|p| p.review_requested).count(),
        ci,
        prs,
        branch,
    }
}

static CACHE: Mutex<Option<(Instant, Overview)>> = Mutex::new(None);

/// The Repos list: repos you worked in lately first.
pub async fn overview(app: &AppHandle, fresh: bool) -> Overview {
    let cached = CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .filter(|(at, _)| at.elapsed() < LIST_TTL)
        .map(|(_, o)| o.clone());
    if !fresh && let Some(o) = cached {
        return o;
    }
    let app2 = app.clone();
    let (repos, via) = tauri::async_runtime::spawn_blocking(move || {
        let mut repos = crate::projects::list(&app2);
        // Most recently touched first.
        repos.sort_by_key(|p| {
            std::cmp::Reverse(
                std::fs::metadata(p.join(".git").join("index"))
                    .and_then(|m| m.modified())
                    .ok(),
            )
        });
        repos.truncate(MAX_REPOS);
        let via = if gh_signed_in() { "gh" } else { "composio" };
        (repos, via)
    })
    .await
    .unwrap_or_default();
    let composio = lock(&app.state::<AppState>().settings).composio.clone();
    let composio_ok = via == "composio" && crate::composio::server(&composio).await.is_some();
    let via = if via == "gh" {
        "gh"
    } else if composio_ok {
        "composio"
    } else {
        "none"
    };
    let mut rows = Vec::new();
    let me = if via == "gh" {
        tauri::async_runtime::spawn_blocking(me)
            .await
            .ok()
            .flatten()
    } else {
        None
    };
    let mut tasks = Vec::new();
    for path in repos {
        let me = me.clone();
        let app = app.clone();
        tasks.push(tauri::async_runtime::spawn(async move {
            // "checked" asks for news: fetch first, and say so if it is new.
            if fresh {
                let (app, path) = (app.clone(), path.clone());
                let _ = tauri::async_runtime::spawn_blocking(move || {
                    crate::git_watch::fetch_and_tell(&app, &path);
                })
                .await;
            }
            let slug = sidekick_sensors::repos::github_url(&path)
                .map(|u| u.trim_start_matches("https://github.com/").to_owned());
            let prs = match (&slug, via) {
                (Some(s), "gh") => {
                    let s = s.clone();
                    tauri::async_runtime::spawn_blocking(move || prs_gh(&s, me.as_deref()))
                        .await
                        .unwrap_or_default()
                }
                (Some(s), "composio") => prs_composio(&app, s).await,
                _ => Vec::new(),
            };
            tauri::async_runtime::spawn_blocking(move || row(&path, slug, prs))
                .await
                .ok()
        }));
    }
    for t in tasks {
        if let Ok(Some(r)) = t.await {
            rows.push(r);
        }
    }
    let o = Overview { via, repos: rows };
    *CACHE.lock().unwrap_or_else(|e| e.into_inner()) = Some((Instant::now(), o.clone()));
    o
}

const QUERY: &str = "query { viewer { pullRequests(last: 25, states: [OPEN, MERGED], orderBy: {field: UPDATED_AT, direction: ASC}) { nodes { number title url state headRefName repository { nameWithOwner } commits(last: 1) { nodes { commit { oid statusCheckRollup { contexts(first: 30) { nodes { ... on CheckRun { name conclusion status detailsUrl } ... on StatusContext { context state targetUrl } } } } } } } } } } search(query: \"is:open is:pr review-requested:@me\", type: ISSUE, first: 20) { nodes { ... on PullRequest { number title url repository { nameWithOwner } } } } }";

/// One notice worth the island, with a key so it is said once.
#[derive(Debug, Clone, PartialEq)]
pub struct Notice {
    pub key: String,
    pub payload: Value,
}

/// What in the GraphQL answer is worth saying.
pub fn notices(v: &Value) -> Vec<Notice> {
    let mut out = Vec::new();
    let data = &v["data"];
    for p in data["viewer"]["pullRequests"]["nodes"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let slug = p["repository"]["nameWithOwner"].as_str().unwrap_or("");
        let base = json!({
            "slug": slug,
            "number": p["number"],
            "title": p["title"],
            "url": p["url"],
            "branch": p["headRefName"],
        });
        let url = p["url"].as_str().unwrap_or("");
        if p["state"] == "MERGED" {
            let mut payload = base.clone();
            payload["kind"] = "merged".into();
            out.push(Notice {
                key: format!("merged:{url}"),
                payload,
            });
            continue;
        }
        let commit = &p["commits"]["nodes"][0]["commit"];
        let checks = commit["statusCheckRollup"]["contexts"]["nodes"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        if let ("fail", Some((check, link))) = rollup(&checks) {
            let mut payload = base.clone();
            payload["kind"] = "ci_failed".into();
            payload["check"] = check.into();
            payload["logs"] = if link.is_empty() {
                url.into()
            } else {
                link.into()
            };
            let oid = commit["oid"].as_str().unwrap_or("");
            out.push(Notice {
                key: format!("ci:{url}:{oid}"),
                payload,
            });
        }
    }
    for p in data["search"]["nodes"].as_array().into_iter().flatten() {
        let url = p["url"].as_str().unwrap_or("");
        if url.is_empty() {
            continue;
        }
        out.push(Notice {
            key: format!("review:{url}"),
            payload: json!({
                "kind": "review",
                "slug": p["repository"]["nameWithOwner"],
                "number": p["number"],
                "title": p["title"],
                "url": url,
            }),
        });
    }
    out
}

/// The local clone of a repo, when there is one.
fn local_path(app: &AppHandle, slug: &str) -> Option<PathBuf> {
    crate::projects::list(app).into_iter().find(|p| {
        sidekick_sensors::repos::github_url(p).is_some_and(|u| {
            u.trim_start_matches("https://github.com/")
                .eq_ignore_ascii_case(slug)
        })
    })
}

pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut said: HashSet<String> = HashSet::new();
        let mut first = true;
        loop {
            let enabled = {
                let state = app.state::<AppState>();
                let s = lock(&state.settings);
                !s.pause.is_active(chrono::Utc::now()) && s.sensor_enabled(ReposSensor::ID)
            };
            if enabled && crate::net::online() {
                let v = tauri::async_runtime::spawn_blocking(|| {
                    gh_signed_in()
                        .then(|| gh(&["api", "graphql", "-f", &format!("query={QUERY}")]))
                        .flatten()
                })
                .await
                .ok()
                .flatten();
                if let Some(v) = v {
                    let found = notices(&v);
                    // What was already true at start is not news.
                    for n in found {
                        if !said.insert(n.key.clone()) || first {
                            continue;
                        }
                        let mut payload = n.payload;
                        let slug = payload["slug"].as_str().unwrap_or("").to_owned();
                        let app2 = app.clone();
                        let path =
                            tauri::async_runtime::spawn_blocking(move || local_path(&app2, &slug))
                                .await
                                .ok()
                                .flatten();
                        payload["has_path"] = path.is_some().into();
                        payload["path"] = path
                            .map(|p| p.to_string_lossy().into_owned())
                            .unwrap_or_default()
                            .into();
                        app.state::<AppState>().bus.publish(Event::new(
                            NOTICE,
                            ReposSensor::ID,
                            payload,
                        ));
                    }
                    first = false;
                }
            }
            tokio::time::sleep(NOTICE_EVERY).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rolls_up_checks() {
        assert_eq!(rollup(&[]).0, "none");
        let pass = json!([{"conclusion":"SUCCESS"},{"state":"SUCCESS"}]);
        assert_eq!(rollup(pass.as_array().unwrap()).0, "pass");
        let pend = json!([{"conclusion":"","status":"IN_PROGRESS"}]);
        assert_eq!(rollup(pend.as_array().unwrap()).0, "pending");
        let fail = json!([{"conclusion":"SUCCESS"},{"name":"lint","conclusion":"FAILURE","detailsUrl":"https://x/1"}]);
        assert_eq!(
            rollup(fail.as_array().unwrap()),
            ("fail", Some(("lint".into(), "https://x/1".into())))
        );
    }

    #[test]
    fn finds_what_is_worth_saying() {
        let v = json!({"data":{
            "viewer":{"pullRequests":{"nodes":[
                {"number":1,"title":"Fix","url":"u1","state":"OPEN","headRefName":"fix","repository":{"nameWithOwner":"a/b"},
                 "commits":{"nodes":[{"commit":{"oid":"c1","statusCheckRollup":{"contexts":{"nodes":[{"name":"test","conclusion":"FAILURE","detailsUrl":"logs"}]}}}}]}},
                {"number":2,"title":"Done","url":"u2","state":"MERGED","headRefName":"done","repository":{"nameWithOwner":"a/b"},"commits":{"nodes":[]}},
                {"number":3,"title":"Green","url":"u3","state":"OPEN","headRefName":"g","repository":{"nameWithOwner":"a/b"},
                 "commits":{"nodes":[{"commit":{"oid":"c3","statusCheckRollup":{"contexts":{"nodes":[{"conclusion":"SUCCESS"}]}}}}]}}
            ]}},
            "search":{"nodes":[{"number":9,"title":"Look","url":"u9","repository":{"nameWithOwner":"c/d"}},{}]}
        }});
        let n = notices(&v);
        let kinds: Vec<&str> = n
            .iter()
            .map(|x| x.payload["kind"].as_str().unwrap())
            .collect();
        assert_eq!(kinds, ["ci_failed", "merged", "review"]);
        assert_eq!(n[0].payload["logs"], "logs");
        assert_eq!(n[0].key, "ci:u1:c1");
    }
}
