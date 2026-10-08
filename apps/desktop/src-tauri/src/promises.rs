//! Promises you made in meetings ("I'll send the deck tomorrow") come back
//! as a reminder the next workday morning. Found in Fathom action items;
//! kept in a small file on this PC.

use std::path::PathBuf;

use chrono::{Datelike, Local, NaiveDate, Timelike, Weekday};
use serde::{Deserialize, Serialize};
use sidekick_skills::{Proposal, ProposedOption, Trust};
use tauri::{AppHandle, Manager};

use crate::state::{AppState, lock};

pub const SKILL: &str = "learn.promise";
const FILE: &str = "promises.json";
const FROM_HOUR: u32 = 9;
const CHECK_EVERY: std::time::Duration = std::time::Duration::from_secs(20 * 60);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Promise {
    pub text: String,
    pub due: NaiveDate,
}

/// The action items that are yours: they name you, or say "I" or "me".
pub fn yours(items: &[String], name: &str) -> Vec<String> {
    let name = name.trim().to_lowercase();
    items
        .iter()
        .filter(|item| {
            let lower = item.to_lowercase();
            let first = lower.split_whitespace().next().unwrap_or("");
            matches!(first, "i" | "i'll" | "i\u{2019}ll" | "i'm" | "me")
                || (!name.is_empty()
                    && lower
                        .split(|c: char| !c.is_alphanumeric())
                        .any(|w| w == name))
        })
        .cloned()
        .collect()
}

/// The next workday after `day`.
pub fn next_workday(day: NaiveDate) -> NaiveDate {
    let mut d = day.succ_opt().unwrap_or(day);
    while matches!(d.weekday(), Weekday::Sat | Weekday::Sun) {
        d = d.succ_opt().unwrap_or(d);
    }
    d
}

fn path(app: &AppHandle) -> PathBuf {
    app.state::<AppState>().data_dir.join(FILE)
}

fn load(app: &AppHandle) -> Vec<Promise> {
    std::fs::read_to_string(path(app))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save(app: &AppHandle, list: &[Promise]) {
    if let Ok(json) = serde_json::to_string(list)
        && let Err(err) = std::fs::write(path(app), json)
    {
        log::warn!("could not save promises: {err}");
    }
}

/// Keeps the user's own action items from a meeting for the next workday.
pub fn from_meeting(app: &AppHandle, items: &[String]) {
    if !crate::learned::on(app) {
        return;
    }
    let name = lock(&app.state::<AppState>().settings).user_name.clone();
    let mine = yours(items, &name);
    if mine.is_empty() {
        return;
    }
    let due = next_workday(Local::now().date_naive());
    let mut list = load(app);
    for text in mine {
        if !list.iter().any(|p| p.text == text) {
            list.push(Promise { text, due });
        }
    }
    save(app, &list);
}

fn card(p: &Promise) -> Proposal {
    let opt = |label: &str, action: &str, args: serde_json::Value| ProposedOption {
        label: label.into(),
        action: action.into(),
        args,
        skill_id: SKILL.into(),
    };
    Proposal {
        skill_id: SKILL.into(),
        skill_ids: vec![SKILL.into()],
        title: "You said you would".into(),
        detail: p.text.clone(),
        options: vec![
            opt(
                "Do it now",
                "ask_ai",
                serde_json::json!({
                    "prompt": format!("Help me do this now: {}", p.text),
                    "clipboard": "false",
                }),
            ),
            opt(
                "Already done",
                "noop",
                serde_json::json!({ "message": "Nice" }),
            ),
        ],
        trust: Trust::Suggest,
        remember: None,
        priority: 60,
    }
}

pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(CHECK_EVERY).await;
            let now = Local::now();
            if now.hour() < FROM_HOUR || !crate::timetrack::is_active(&app) {
                continue;
            }
            let today = now.date_naive();
            let (due, later): (Vec<Promise>, Vec<Promise>) =
                load(&app).into_iter().partition(|p| p.due <= today);
            if due.is_empty() {
                continue;
            }
            save(&app, &later);
            for p in &due {
                crate::suggestions::offer(&app, card(p));
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_your_promises() {
        let items = vec![
            "Sara sends the mocks".to_string(),
            "Sam to share the budget by Friday".to_string(),
            "I'll send the deck tomorrow".to_string(),
        ];
        assert_eq!(
            yours(&items, "Sam"),
            vec![
                "Sam to share the budget by Friday",
                "I'll send the deck tomorrow"
            ]
        );
        assert_eq!(yours(&items, ""), vec!["I'll send the deck tomorrow"]);
    }

    #[test]
    fn reminds_on_the_next_workday() {
        let fri = NaiveDate::from_ymd_opt(2026, 10, 9).unwrap();
        assert_eq!(
            next_workday(fri),
            NaiveDate::from_ymd_opt(2026, 10, 12).unwrap()
        );
        let tue = NaiveDate::from_ymd_opt(2026, 10, 6).unwrap();
        assert_eq!(
            next_workday(tue),
            NaiveDate::from_ymd_opt(2026, 10, 7).unwrap()
        );
    }
}
