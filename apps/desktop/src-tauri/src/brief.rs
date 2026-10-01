//! Morning brief (FR-COMM-01): the first time you are at the computer each
//! morning, one card with yesterday's time, repos with unsaved work and, when
//! the GitHub CLI is signed in, pull requests waiting on you. Everything is
//! gathered on this PC; nothing is sent anywhere unless you ask AI to plan
//! the day with it.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use chrono::{Local, Timelike};
use serde_json::Value;
use sidekick_core::{AppTime, Event};
use tauri::{AppHandle, Manager};

use crate::state::{AppState, lock};
use crate::timetrack;

pub const MORNING_BRIEF: &str = "day.morning_brief";
/// No brief before this local hour (a late night session is not a morning).
const EARLIEST_HOUR: u32 = 5;
const CHECK_EVERY: Duration = Duration::from_secs(60);
const GH_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_LIST: usize = 5;

#[derive(Debug, Clone, PartialEq)]
pub struct Pr {
    pub repo: String,
    pub title: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Repo {
    pub name: String,
    pub changed: usize,
    pub ahead: usize,
}

/// Builds the brief, or None when there is nothing worth saying.
/// `meetings` are today's, as (local start time, title).
pub fn compose(
    yesterday: &[AppTime],
    meetings: &[(String, String)],
    repos: &[Repo],
    reviews: &[Pr],
    mine: &[Pr],
) -> Option<Event> {
    let mut parts = Vec::new();
    let mut lines = Vec::new();

    let total: i64 = yesterday.iter().map(|r| r.secs).sum();
    if total >= 30 * 60 {
        let top = timetrack::by_name(yesterday);
        parts.push(format!("Yesterday {}", timetrack::human(total)));
        lines.push(format!(
            "Yesterday: {} ({})",
            timetrack::human(total),
            top.iter()
                .take(3)
                .map(|(n, s)| format!("{n} {}", timetrack::human(*s)))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !meetings.is_empty() {
        parts.push(plural(meetings.len(), "meeting", "meetings"));
        lines.push("Meetings today:".into());
        for (at, title) in meetings {
            lines.push(format!("- {at} {title}"));
        }
    }
    if !repos.is_empty() {
        parts.push(plural(repos.len(), "repo", "repos") + " unsaved");
        lines.push("Unsaved work:".into());
        for r in repos.iter().take(MAX_LIST) {
            lines.push(format!(
                "- {}: {} changed, {} unpushed",
                r.name, r.changed, r.ahead
            ));
        }
    }
    if !reviews.is_empty() {
        parts.push(plural(reviews.len(), "review", "reviews") + " waiting");
        lines.push("Reviews waiting on you:".into());
        lines.extend(reviews.iter().take(MAX_LIST).map(pr_line));
    }
    if !mine.is_empty() {
        parts.push(plural(mine.len(), "open PR", "open PRs"));
        lines.push("Your open PRs:".into());
        lines.extend(mine.iter().take(MAX_LIST).map(pr_line));
    }
    if parts.is_empty() {
        return None;
    }
    let first_url = reviews.first().or(mine.first()).map(|p| p.url.clone());
    Some(Event::new(
        MORNING_BRIEF,
        "time",
        serde_json::json!({
            "headline": parts.join(" · "),
            "text": lines.join("\n"),
            "reviews": reviews.len(),
            "first_url": first_url.unwrap_or_default(),
            "first_title": reviews.first().or(mine.first()).map(|p| p.title.clone()).unwrap_or_default(),
        }),
    ))
}

fn pr_line(p: &Pr) -> String {
    format!("- {}: {} ({})", p.repo, p.title, p.url)
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// Parses `gh search prs --json title,url,repository`.
pub fn parse_prs(json: &str) -> Vec<Pr> {
    let Ok(Value::Array(items)) = serde_json::from_str::<Value>(json) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|i| {
            let url = i["url"].as_str()?;
            // Only links to GitHub go on a button.
            if !url.starts_with("https://github.com/") {
                return None;
            }
            Some(Pr {
                repo: i["repository"]["nameWithOwner"]
                    .as_str()
                    .or(i["repository"]["name"].as_str())
                    .unwrap_or_default()
                    .to_owned(),
                title: i["title"].as_str().unwrap_or_default().to_owned(),
                url: url.to_owned(),
            })
        })
        .collect()
}

/// Runs the GitHub CLI with fixed arguments. Missing, signed out, offline or
/// slow all mean "no PRs".
fn gh(filter: &str) -> Vec<Pr> {
    let mut cmd = Command::new("gh");
    cmd.args([
        "search",
        "prs",
        filter,
        "--state=open",
        "--json",
        "title,url,repository",
        "--limit",
        "10",
    ])
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let Ok(mut child) = cmd.spawn() else {
        return Vec::new();
    };
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) | Err(_) => return Vec::new(),
            Ok(None) if started.elapsed() > GH_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return Vec::new();
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
        }
    }
    let mut out = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        let _ = std::io::Read::read_to_string(&mut stdout, &mut out);
    }
    parse_prs(&out)
}

fn unsaved_repos(roots: &[PathBuf]) -> Vec<Repo> {
    roots
        .iter()
        .flat_map(|r| sidekick_sensors::repos::find_repos(r))
        .filter_map(|repo| {
            let (changed, ahead) = sidekick_sensors::repos::unsaved(&repo)?;
            Some(Repo {
                name: repo.file_name()?.to_string_lossy().into_owned(),
                changed,
                ahead,
            })
        })
        .collect()
}

fn gather(app: &AppHandle, roots: &[PathBuf]) -> Option<Event> {
    let yesterday = (Local::now() - chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();
    let rows = lock(&app.state::<AppState>().storage)
        .time_for_day(&yesterday)
        .unwrap_or_default();
    let meetings: Vec<(String, String)> = {
        let state = app.state::<AppState>();
        let c = state
            .calendar
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        sidekick_sensors::calendar::on_day(&c.meetings, Local::now().date_naive())
            .iter()
            .map(|m| {
                (
                    m.start.with_timezone(&Local).format("%H:%M").to_string(),
                    m.title.clone(),
                )
            })
            .collect()
    };
    let repos = unsaved_repos(roots);
    let reviews = gh("--review-requested=@me");
    let mine = gh("--author=@me");
    compose(&rows, &meetings, &repos, &reviews, &mine)
}

/// The day the last brief was shown, kept in a small file so a restart does
/// not repeat it.
fn last_day(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .trim()
        .to_owned()
}

pub fn start(app: &AppHandle, marker: PathBuf, roots: Vec<PathBuf>) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(CHECK_EVERY).await;
            let now = Local::now();
            let today = now.format("%Y-%m-%d").to_string();
            if now.hour() < EARLIEST_HOUR || last_day(&marker) == today {
                continue;
            }
            let state = app.state::<AppState>();
            // Only once you are actually here.
            if state.away.load(std::sync::atomic::Ordering::Relaxed) || !timetrack::is_active(&app)
            {
                continue;
            }
            let _ = std::fs::write(&marker, &today);
            let (app2, roots2) = (app.clone(), roots.clone());
            let event = tokio::task::spawn_blocking(move || gather(&app2, &roots2))
                .await
                .ok()
                .flatten();
            if let Some(e) = event {
                app.state::<AppState>().bus.publish(e);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pr(repo: &str, title: &str) -> Pr {
        Pr {
            repo: repo.into(),
            title: title.into(),
            url: format!("https://github.com/{repo}/pull/1"),
        }
    }

    #[test]
    fn composes_a_brief() {
        let rows = [AppTime {
            app: "Code".into(),
            project: "sidekick".into(),
            secs: 5400,
        }];
        let repos = [Repo {
            name: "api".into(),
            changed: 3,
            ahead: 1,
        }];
        let meetings = [("15:00".to_owned(), "Design review".to_owned())];
        let e = compose(&rows, &meetings, &repos, &[pr("me/api", "Fix login")], &[]).unwrap();
        assert_eq!(
            e.payload["headline"],
            "Yesterday 1 h 30 min · 1 meeting · 1 repo unsaved · 1 review waiting"
        );
        let text = e.payload["text"].as_str().unwrap();
        assert!(text.contains("- api: 3 changed, 1 unpushed"));
        assert!(text.contains("- 15:00 Design review"));
        assert!(text.contains("- me/api: Fix login (https://github.com/me/api/pull/1)"));
        assert_eq!(e.payload["first_url"], "https://github.com/me/api/pull/1");
    }

    #[test]
    fn nothing_to_say_is_no_brief() {
        let short = [AppTime {
            app: "Code".into(),
            project: String::new(),
            secs: 600,
        }];
        assert!(compose(&short, &[], &[], &[], &[]).is_none());
    }

    #[test]
    fn parses_gh_output_and_drops_odd_links() {
        let json = r#"[
            {"title":"Add brief","url":"https://github.com/me/sidekick/pull/6","repository":{"name":"sidekick","nameWithOwner":"me/sidekick"}},
            {"title":"Bad","url":"javascript:alert(1)","repository":{"name":"x"}}
        ]"#;
        let prs = parse_prs(json);
        assert_eq!(
            prs,
            vec![Pr {
                repo: "me/sidekick".into(),
                title: "Add brief".into(),
                url: "https://github.com/me/sidekick/pull/6".into(),
            }]
        );
        assert!(parse_prs("not json").is_empty());
    }
}
