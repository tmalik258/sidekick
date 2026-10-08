//! Everything Sidekick has learned from your choices, in plain words, for
//! the Memory tab: picks it remembers, suggestions resting after you
//! dismissed them, and routines. Each one can be forgotten.

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::state::{AppState, lock};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Learned {
    /// "choice", "quiet" or "routine": how to forget it.
    pub kind: String,
    pub key: String,
    pub label: String,
    /// What was learned, as a sentence.
    pub text: String,
    /// Why Sidekick thinks so.
    pub why: String,
}

/// What a remembered pick is about, from its key ("url", "url:localhost").
fn about(key: &str) -> String {
    let (kind, scope) = key.split_once(':').unwrap_or((key, ""));
    let what = match kind {
        "url" => "links",
        "file" | "download" => "files",
        "screenshot" => "screenshots",
        "editor" => "projects",
        other => other,
    };
    if scope.is_empty() {
        format!("For {what}")
    } else {
        format!("For {what} from {scope}")
    }
}

pub fn list(app: &AppHandle) -> Vec<Learned> {
    let state = app.state::<AppState>();
    let storage = lock(&state.storage);
    let mut out = Vec::new();
    for (key, label, count, _) in storage.all_choices().unwrap_or_default() {
        if count < 2 {
            continue;
        }
        out.push(Learned {
            kind: "choice".into(),
            text: format!("{}, you pick {label}", about(&key)),
            why: format!("{count} times"),
            key,
            label,
        });
    }
    let now = chrono::Utc::now().to_rfc3339();
    for (skill, until) in storage.muted_habits(&now).unwrap_or_default() {
        let day = chrono::DateTime::parse_from_rfc3339(&until)
            .map(|d| {
                d.with_timezone(&chrono::Local)
                    .format("%A %H:%M")
                    .to_string()
            })
            .unwrap_or(until);
        out.push(Learned {
            kind: "quiet".into(),
            text: format!("{} suggestions are resting", skill_name(&skill)),
            why: format!("You dismissed them 3 times in a row. Back {day}."),
            key: skill,
            label: String::new(),
        });
    }
    drop(storage);
    for item in crate::routines::today(app) {
        out.push(Learned {
            kind: "routine".into(),
            text: format!("You open {} as part of your day", item.label),
            why: format!("On {} of the last days", item.days),
            key: item.key,
            label: item.kind,
        });
    }
    out
}

/// "clipboard.open-url" to "Open copied links"-ish: the last part, spaced.
fn skill_name(id: &str) -> String {
    let last = id.rsplit('.').next().unwrap_or(id).replace(['-', '_'], " ");
    let mut c = last.chars();
    c.next()
        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
        .unwrap_or_default()
}

pub fn forget(app: &AppHandle, kind: &str, key: &str, label: &str) -> Result<(), String> {
    match kind {
        "choice" => lock(&app.state::<AppState>().storage)
            .forget_choice(key, label)
            .map(|_| ())
            .map_err(|e| e.to_string()),
        "quiet" => lock(&app.state::<AppState>().storage)
            .clear_habit(key)
            .map(|_| ())
            .map_err(|e| e.to_string()),
        "routine" => crate::routines::remove(app, label, key).map(|_| ()),
        _ => Err(format!("unknown kind {kind}")),
    }
}

pub fn forget_all(app: &AppHandle) -> Result<(), String> {
    {
        let state = app.state::<AppState>();
        let storage = lock(&state.storage);
        storage.clear_choices().map_err(|e| e.to_string())?;
        storage.clear_habits().map_err(|e| e.to_string())?;
    }
    crate::routines::forget(app).map(|_| ())
}

/// Whether to learn from what just happened.
pub fn on(app: &AppHandle) -> bool {
    lock(&app.state::<AppState>().settings).learning
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn says_what_a_pick_is_about() {
        assert_eq!(about("url"), "For links");
        assert_eq!(about("url:localhost"), "For links from localhost");
        assert_eq!(skill_name("clipboard.open-url"), "Open url");
    }
}

/// Once a week (Friday afternoon), Sidekick shows a few of its guesses
/// about you so you can fix the wrong ones in Memory.
pub const WEEKLY_SKILL: &str = "learn.weekly";
const WEEKLY_DAY: chrono::Weekday = chrono::Weekday::Fri;
const WEEKLY_FROM_HOUR: u32 = 14;
const WEEKLY_CHECK: std::time::Duration = std::time::Duration::from_secs(30 * 60);

/// The card: the three newest-looking guesses, or None when there are none.
pub fn weekly_card(items: &[Learned]) -> Option<sidekick_skills::Proposal> {
    use sidekick_skills::{Proposal, ProposedOption, Trust};
    if items.is_empty() {
        return None;
    }
    let lines: Vec<&str> = items.iter().take(3).map(|l| l.text.as_str()).collect();
    let more = items.len().saturating_sub(3);
    let detail = if more > 0 {
        format!("{}. And {more} more.", lines.join(". "))
    } else {
        format!("{}.", lines.join(". "))
    };
    let opt = |label: &str, action: &str| ProposedOption {
        label: label.into(),
        action: action.into(),
        args: serde_json::json!({ "message": "OK" }),
        skill_id: WEEKLY_SKILL.into(),
    };
    Some(Proposal {
        skill_id: WEEKLY_SKILL.into(),
        skill_ids: vec![WEEKLY_SKILL.into()],
        title: "What I learned about you this week".into(),
        detail,
        options: vec![
            opt("Review and fix", "open_memory"),
            opt("Looks right", "noop"),
        ],
        trust: Trust::Suggest,
        remember: None,
        priority: 50,
    })
}

pub fn start_weekly(app: &AppHandle, marker: std::path::PathBuf) {
    use chrono::{Datelike, Timelike};
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(WEEKLY_CHECK).await;
            let now = chrono::Local::now();
            let week = format!("{}-{}", now.iso_week().year(), now.iso_week().week());
            if now.weekday() != WEEKLY_DAY
                || now.hour() < WEEKLY_FROM_HOUR
                || std::fs::read_to_string(&marker).unwrap_or_default() == week
                || !on(&app)
                || !crate::timetrack::is_active(&app)
            {
                continue;
            }
            let _ = std::fs::write(&marker, &week);
            let app2 = app.clone();
            let items = tokio::task::spawn_blocking(move || list(&app2))
                .await
                .unwrap_or_default();
            if let Some(card) = weekly_card(&items) {
                crate::suggestions::offer(&app, card);
            }
        }
    });
}

#[cfg(test)]
mod weekly_tests {
    use super::*;

    fn item(text: &str) -> Learned {
        Learned {
            kind: "choice".into(),
            key: String::new(),
            label: String::new(),
            text: text.into(),
            why: String::new(),
        }
    }

    #[test]
    fn weekly_card_lists_three_guesses() {
        assert!(weekly_card(&[]).is_none());
        let items: Vec<Learned> = ["A", "B", "C", "D", "E"].map(item).into();
        let card = weekly_card(&items).unwrap();
        assert_eq!(card.detail, "A. B. C. And 2 more.");
        assert_eq!(card.options[0].action, "open_memory");
    }
}
