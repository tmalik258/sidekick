//! Small moments in the day that each get one card: back from a break,
//! a meeting about to start, late at night, the end of the day and Friday
//! afternoon. Each is a skill, on by default and switchable in Settings.

use std::collections::VecDeque;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};

use chrono::{Datelike, Local, NaiveDate, Timelike, Weekday};
use serde_json::Value;
use sidekick_core::{AppTime, Event};
use tauri::{AppHandle, Manager};

use crate::state::{AppState, lock};
use crate::timetrack;

pub const BACK: &str = "day.back";
pub const LATE_NIGHT: &str = "time.late_night";
pub const WEEK_SUMMARY: &str = "time.week_summary";

/// A break shorter than this needs no recap; longer than the cap is a new
/// session (the morning card covers that).
const BREAK_MIN_SECS: u64 = 10 * 60;
const BREAK_MAX_SECS: u64 = 4 * 60 * 60;
/// Windows remembered for the recap.
const KEEP_WINDOWS: usize = 8;
/// Late night is from this hour until 4 am.
const LATE_FROM: u32 = 23;
/// Friday's summary comes after this hour.
const WEEK_FROM: u32 = 15;

/// Recent windows as (app, title), newest last.
static WINDOWS: LazyLock<Mutex<VecDeque<(String, String)>>> =
    LazyLock::new(|| Mutex::new(VecDeque::new()));

/// Remembers the window that came to the front, for "Where was I".
pub fn on_window(payload: &Value) {
    let app = payload["app"].as_str().unwrap_or_default().trim();
    let title = payload["title"].as_str().unwrap_or_default().trim();
    let exe = payload["exe"].as_str().unwrap_or_default();
    if app.is_empty() || title.is_empty() {
        return;
    }
    // Lock screen is not somewhere you were working.
    if sidekick_sensors::is_lock_ui(exe) || sidekick_sensors::is_lock_ui(app) {
        return;
    }
    if let Ok(mut w) = WINDOWS.lock() {
        let item = (app.to_owned(), title.to_owned());
        w.retain(|x| *x != item);
        w.push_back(item);
        while w.len() > KEEP_WINDOWS {
            w.pop_front();
        }
    }
}

/// The "Where was I" card, when a break was long enough to need one.
pub fn back_event(
    away_secs: u64,
    windows: &[(String, String)],
    page: Option<(String, String)>,
    chat: Option<String>,
) -> Option<Event> {
    if !(BREAK_MIN_SECS..=BREAK_MAX_SECS).contains(&away_secs) || windows.is_empty() {
        return None;
    }
    let (app, title) = windows.last()?;
    let mut recap = vec!["Before my break I had these windows open (newest last):".to_owned()];
    recap.extend(windows.iter().map(|(a, t)| format!("- {a}: {t}")));
    let (page_title, page_url) = page.unwrap_or_default();
    if !page_title.is_empty() {
        recap.push(format!("Last page I read: {page_title} ({page_url})"));
    }
    let chat = chat.unwrap_or_default();
    if !chat.is_empty() {
        recap.push(format!("Last Sidekick chat: {chat}"));
    }
    let short: String = title.chars().take(70).collect();
    Some(Event::new(
        BACK,
        "time",
        serde_json::json!({
            "minutes": away_secs / 60,
            "app": app,
            "title": short,
            "page_title": page_title,
            "page_url": page_url,
            "recap": recap.join("\n"),
        }),
    ))
}

/// Called when the user is back at the computer.
pub fn on_back(app: &AppHandle, away_secs: u64) {
    let windows: Vec<(String, String)> = WINDOWS
        .lock()
        .map(|w| w.iter().cloned().collect())
        .unwrap_or_default();
    let state = app.state::<AppState>();
    let (page, chat) = {
        let storage = lock(&state.storage);
        let page = storage
            .recent_items("page", 1)
            .ok()
            .and_then(|v| v.into_iter().next())
            .map(|(url, title, _, _)| (title, url));
        let chat = storage
            .recent_chats(1)
            .ok()
            .and_then(|v| v.into_iter().next())
            .map(|c| c.title);
        (page, chat)
    };
    if let Some(e) = back_event(away_secs, &windows, page, chat) {
        state.bus.publish(e);
    }
}

/// Adds the page or file that best matches a meeting about to start, so
/// its card can open what you used last time.
pub fn enrich_meeting(app: &AppHandle, event: &mut Event) {
    let title = event.payload["title"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    if title.trim().is_empty() {
        return;
    }
    let hit = lock(&app.state::<AppState>().storage)
        .search(&title, &["page", "file"], 1)
        .ok()
        .and_then(|v| v.into_iter().next());
    if let (Some(hit), Some(obj)) = (hit, event.payload.as_object_mut()) {
        let short: String = hit.title.chars().take(40).collect();
        obj.insert("doc_title".into(), short.into());
        obj.insert("doc_source".into(), hit.source.clone().into());
        obj.insert(
            if hit.source == "page" {
                "doc_url"
            } else {
                "doc_path"
            }
            .into(),
            hit.reference.into(),
        );
    }
}

/// Whether `hour` is late at night.
pub fn is_late(hour: u32) -> bool {
    !(4..LATE_FROM).contains(&hour)
}

/// The week so far, Monday to `today`, as one event.
pub fn week_summary(days: &[(NaiveDate, Vec<AppTime>)]) -> Option<Event> {
    let rows: Vec<AppTime> = days.iter().flat_map(|(_, r)| r.iter().cloned()).collect();
    let total: i64 = rows.iter().map(|r| r.secs).sum();
    if total < 2 * 60 * 60 {
        return None;
    }
    let by_name = timetrack::by_name(&rows);
    let top: Vec<String> = by_name
        .iter()
        .take(3)
        .map(|(n, s)| format!("{n} {}", timetrack::human(*s)))
        .collect();
    let mut lines = vec![format!("This week ({} total)", timetrack::human(total))];
    for (day, r) in days {
        let secs: i64 = r.iter().map(|x| x.secs).sum();
        if secs > 0 {
            lines.push(format!(
                "- {}: {}",
                day.format("%a"),
                timetrack::human(secs)
            ));
        }
    }
    lines.push("By project:".into());
    lines.extend(
        by_name
            .iter()
            .filter(|(_, s)| *s >= 15 * 60)
            .map(|(n, s)| format!("- {n}: {}", timetrack::human(*s))),
    );
    Some(Event::new(
        WEEK_SUMMARY,
        "time",
        serde_json::json!({
            "total_human": timetrack::human(total),
            "top": top.join(", "),
            "text": lines.join("\n"),
        }),
    ))
}

/// Late night once a night, the week's summary on Friday afternoon.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut late_on: Option<NaiveDate> = None;
        let mut week_on: Option<NaiveDate> = None;
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            let state = app.state::<AppState>();
            if state.away.load(std::sync::atomic::Ordering::Relaxed)
                || !timetrack::is_active(&app)
                || crate::state::is_paused(&app)
            {
                continue;
            }
            let now = Local::now();
            // A night belongs to the evening it started.
            let night = if now.hour() < 4 {
                now.date_naive().pred_opt().unwrap_or(now.date_naive())
            } else {
                now.date_naive()
            };
            if is_late(now.hour()) && late_on != Some(night) {
                late_on = Some(night);
                // Only offer what is not on already (night light on a
                // schedule reads as on at this hour).
                let pc = tokio::task::spawn_blocking(sidekick_actions::pc::read_state)
                    .await
                    .unwrap_or_default();
                state.bus.publish(Event::new(
                    LATE_NIGHT,
                    "time",
                    serde_json::json!({
                        "time": now.format("%H:%M").to_string(),
                        "night_light": pc.night_light.as_str(),
                        "dnd": pc.do_not_disturb.as_str(),
                    }),
                ));
            }
            let today = now.date_naive();
            if today.weekday() == Weekday::Fri && now.hour() >= WEEK_FROM && week_on != Some(today)
            {
                week_on = Some(today);
                let monday = today - chrono::Duration::days(4);
                let days: Vec<(NaiveDate, Vec<AppTime>)> = monday
                    .iter_days()
                    .take(5)
                    .map(|d| {
                        let rows = lock(&state.storage)
                            .time_for_day(&d.format("%Y-%m-%d").to_string())
                            .unwrap_or_default();
                        (d, rows)
                    })
                    .collect();
                if let Some(e) = week_summary(&days) {
                    state.bus.publish(e);
                }
            }
        }
    });
}

fn worklog_path() -> Option<PathBuf> {
    dirs::document_dir().map(|d| d.join("Sidekick").join("worklog.md"))
}

/// Appends a dated entry to Documents\Sidekick\worklog.md.
pub fn save_log(text: &str) -> Result<(String, String), String> {
    let path = worklog_path().ok_or("No Documents folder")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| e.to_string())?;
    let stamp = Local::now().format("%A %-d %B %Y, %H:%M");
    writeln!(f, "\n## {stamp}\n\n{}\n", text.trim()).map_err(|e| e.to_string())?;
    Ok(("Added to your work log".into(), path.display().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(app: &str, title: &str) -> (String, String) {
        (app.into(), title.into())
    }

    #[test]
    fn recaps_a_real_break_only() {
        let windows = [
            w("Code", "main.rs - sidekick"),
            w("Chrome", "PR 16 - GitHub"),
        ];
        assert!(back_event(120, &windows, None, None).is_none());
        assert!(back_event(5 * 60 * 60, &windows, None, None).is_none());
        let e = back_event(
            25 * 60,
            &windows,
            Some(("Rust book".into(), "https://doc.rust-lang.org".into())),
            Some("Fix port 3000".into()),
        )
        .unwrap();
        assert_eq!(e.payload["minutes"], 25);
        assert_eq!(e.payload["app"], "Chrome");
        let recap = e.payload["recap"].as_str().unwrap();
        assert!(recap.contains("- Code: main.rs - sidekick"));
        assert!(recap.contains("Last Sidekick chat: Fix port 3000"));
    }

    #[test]
    fn late_is_eleven_to_four() {
        assert!(is_late(23));
        assert!(is_late(2));
        assert!(!is_late(4));
        assert!(!is_late(22));
    }

    #[test]
    fn sums_the_week() {
        let day = |d| NaiveDate::from_ymd_opt(2026, 9, d).unwrap();
        let row = |p: &str, secs| AppTime {
            app: "Code".into(),
            project: p.into(),
            secs,
        };
        let days = vec![
            (day(28), vec![row("sidekick", 3 * 3600)]),
            (day(29), vec![]),
            (day(30), vec![row("api", 3600), row("sidekick", 1800)]),
        ];
        let e = week_summary(&days).unwrap();
        assert_eq!(e.payload["total_human"], "4 h 30 min");
        let text = e.payload["text"].as_str().unwrap();
        assert!(text.contains("- Mon: 3 h 0 min"));
        assert!(!text.contains("Tue"), "days with no time are left out");
        assert!(text.contains("- sidekick: 3 h 30 min"));
        assert!(week_summary(&days[1..2]).is_none());
    }
}
