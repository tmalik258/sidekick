//! Recipes: a task the user did once, saved to run again by name or on a
//! trigger (a time, a notification, a download, a meeting ending, an app
//! opening). A due recipe asks first with a card, unless the user let it
//! start alone; either way anything that sends, posts or pays still waits
//! for a tap, because a recipe runs as an ordinary Ask question.
//!
//! Also here: noticing a question asked three times and offering to save it.

use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use chrono::{Datelike, Local};
use serde_json::{Value, json};
use sidekick_core::{Event, Recipe, Trigger};
use tauri::{AppHandle, Manager};

use crate::state::{AppState, lock};

pub const DUE: &str = "recipe.due";
pub const OFFER: &str = "recipe.offer";
const TICK: Duration = Duration::from_secs(20);
/// Asking the same thing this many times offers to save it.
const ASKED_TO_OFFER: u32 = 3;
const MAX_RECIPES: usize = 50;

/// Recipes already fired, by "id|day|why", so one trigger fires once.
static FIRED: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));
/// How often each question was asked (normalized).
static ASKED: LazyLock<Mutex<HashMap<String, u32>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

const DAYS: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

/// Whether a time trigger is due at `hh_mm` on `day` ("mon".."sun").
pub fn time_due(time: &str, days: &[String], hh_mm: &str, day: &str) -> bool {
    let norm = |t: &str| {
        let (h, m) = t.trim().split_once(':').unwrap_or((t, "0"));
        format!(
            "{:02}:{:02}",
            h.trim().parse::<u32>().unwrap_or(99),
            m.trim().parse::<u32>().unwrap_or(99)
        )
    };
    norm(time) == hh_mm
        && (days.is_empty()
            || days
                .iter()
                .any(|d| d.trim().to_lowercase().starts_with(day)))
}

/// Whether an event matches a trigger. Returns why, for the card.
pub fn event_matches(trigger: &Trigger, kind: &str, payload: &Value) -> Option<String> {
    let text = |k: &str| payload[k].as_str().unwrap_or_default().to_lowercase();
    match trigger {
        Trigger::Download { kind: want } if kind == "file.download_completed" => {
            let want = want.trim().to_lowercase();
            (want.is_empty() || text("kind") == want || text("ext") == want).then(|| {
                format!(
                    "{} downloaded",
                    payload["name"].as_str().unwrap_or("A file")
                )
            })
        }
        Trigger::MeetingEnded if kind == "calendar.meeting_ended" => Some(format!(
            "{} ended",
            payload["title"].as_str().unwrap_or("A meeting")
        )),
        Trigger::AppOpened { app } if kind == "window.focused" => {
            let want = app.trim().to_lowercase();
            (!want.is_empty() && text("app").contains(&want)).then(|| format!("{app} opened"))
        }
        _ => None,
    }
}

/// A notification against a recipe's trigger.
pub fn notification_matches(trigger: &Trigger, from: &str, title: &str, body: &str) -> bool {
    let Trigger::Notification { app, contains } = trigger else {
        return false;
    };
    let app_ok = app.trim().is_empty() || from.to_lowercase().contains(&app.trim().to_lowercase());
    let word = contains.trim().to_lowercase();
    let text_ok = word.is_empty() || format!("{title} {body}").to_lowercase().contains(&word);
    app_ok && text_ok
}

fn recipes(app: &AppHandle) -> Vec<Recipe> {
    lock(&app.state::<AppState>().settings).recipes.clone()
}

/// Fires a recipe once per key (so a minute's ticks or repeated events do
/// not repeat it).
fn fire(app: &AppHandle, r: &Recipe, why: &str, key: &str) {
    if !lock(&FIRED).insert(format!("{}|{key}", r.id)) {
        return;
    }
    let paused = lock(&app.state::<AppState>().settings)
        .pause
        .is_active(chrono::Utc::now());
    if paused {
        return;
    }
    if r.auto {
        run(app, r);
        return;
    }
    app.state::<AppState>().bus.publish(Event::new(
        DUE,
        "recipes",
        json!({ "id": r.id, "name": r.name, "why": why }),
    ));
}

/// Runs a recipe as an Ask question, so it shows its steps and its taps.
pub fn run(app: &AppHandle, r: &Recipe) {
    crate::ask::open(
        app,
        crate::ask::Open {
            prompt: Some(r.prompt.clone()),
            ask: true,
            clipboard: false,
            ..Default::default()
        },
    );
}

pub fn run_by_id(app: &AppHandle, id: &str) -> Result<String, String> {
    let r = recipes(app)
        .into_iter()
        .find(|r| r.id == id || r.name.eq_ignore_ascii_case(id))
        .ok_or("that recipe is gone")?;
    run(app, &r);
    Ok(format!("Running {}", r.name))
}

pub fn start(app: &AppHandle) {
    // Time triggers.
    let a = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(TICK).await;
            let now = Local::now();
            let hh_mm = now.format("%H:%M").to_string();
            let day = DAYS[now.weekday().num_days_from_monday() as usize];
            for r in recipes(&a).iter().filter(|r| r.enabled) {
                if let Trigger::Time { time, days } = &r.trigger
                    && time_due(time, days, &hh_mm, day)
                {
                    fire(
                        &a,
                        r,
                        &format!("It's {hh_mm}"),
                        &format!("{} {hh_mm}", now.date_naive()),
                    );
                }
            }
        }
    });
    // Event triggers.
    let a = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut rx = a.state::<AppState>().bus.subscribe();
        loop {
            let event = match rx.recv().await {
                Ok(e) => e,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            };
            for r in recipes(&a).iter().filter(|r| r.enabled) {
                if let Some(why) = event_matches(&r.trigger, &event.kind, &event.payload) {
                    // Apps once a day; the rest once per event.
                    let key = if matches!(r.trigger, Trigger::AppOpened { .. }) {
                        Local::now().date_naive().to_string()
                    } else {
                        event.id.to_string()
                    };
                    fire(&a, r, &why, &key);
                }
            }
        }
    });
}

/// Called by the notification inbox for every notification it keeps.
pub fn on_notification(app: &AppHandle, id: i64, from: &str, title: &str, body: &str) {
    for r in recipes(app).iter().filter(|r| r.enabled) {
        if notification_matches(&r.trigger, from, title, body) {
            fire(app, r, &format!("{from}: {title}"), &format!("n{id}"));
        }
    }
}

fn normalize(q: &str) -> String {
    q.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// "run Send timesheet" or the recipe's name alone runs its instruction.
pub fn expand(app: &AppHandle, question: &str) -> Option<String> {
    let q = normalize(question);
    let q = q.strip_prefix("run ").unwrap_or(&q);
    recipes(app)
        .into_iter()
        .find(|r| r.enabled && !r.name.is_empty() && normalize(&r.name) == q)
        .map(|r| r.prompt)
}

/// Counts a question; the third time, offers to save it as a recipe.
pub fn note_question(app: &AppHandle, question: &str) {
    let key = normalize(question);
    if key.split(' ').count() < 3 {
        return;
    }
    let n = {
        let mut asked = lock(&ASKED);
        let n = asked.entry(key.clone()).or_default();
        *n += 1;
        *n
    };
    let known = recipes(app).iter().any(|r| normalize(&r.prompt) == key);
    if n == ASKED_TO_OFFER && !known {
        app.state::<AppState>().bus.publish(Event::new(
            OFFER,
            "recipes",
            json!({ "prompt": question.trim(), "name": short_name(question) }),
        ));
    }
}

/// A recipe name from its instruction: the first few words.
pub fn short_name(prompt: &str) -> String {
    let words: Vec<&str> = prompt.split_whitespace().take(6).collect();
    let mut name = words.join(" ");
    name = name.trim_end_matches(['.', '?', '!', ',']).to_owned();
    let mut c = name.chars();
    c.next()
        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
        .unwrap_or_default()
}

/// Saves a recipe (new, or the same name replaced).
pub fn save(app: &AppHandle, mut r: Recipe) -> Result<String, String> {
    if r.prompt.trim().is_empty() {
        return Err("a recipe needs an instruction".into());
    }
    if r.name.trim().is_empty() {
        r.name = short_name(&r.prompt);
    }
    if r.id.is_empty() {
        r.id = ulid::Ulid::new().to_string();
    }
    let mut next = lock(&app.state::<AppState>().settings).clone();
    next.recipes
        .retain(|x| x.id != r.id && !x.name.eq_ignore_ascii_case(&r.name));
    if next.recipes.len() >= MAX_RECIPES {
        return Err(format!(
            "{MAX_RECIPES} recipes is the most; delete one first"
        ));
    }
    let name = r.name.clone();
    next.recipes.push(r);
    crate::commands::apply_settings(app, next)?;
    Ok(format!("Saved recipe {name}"))
}

pub fn delete(app: &AppHandle, id_or_name: &str) -> Result<String, String> {
    let mut next = lock(&app.state::<AppState>().settings).clone();
    let before = next.recipes.len();
    next.recipes
        .retain(|r| r.id != id_or_name && !r.name.eq_ignore_ascii_case(id_or_name));
    if next.recipes.len() == before {
        return Err("no recipe by that name".into());
    }
    crate::commands::apply_settings(app, next)?;
    Ok("Deleted".into())
}

/// A trigger from a model's plain fields: when = "time" with time and
/// days, "notification" with app and contains, "download" with kind,
/// "meeting_ended", "app_opened" with app, or "manual".
pub fn trigger_from(v: &Value) -> Trigger {
    let s = |k: &str| v[k].as_str().unwrap_or_default().to_owned();
    match v["when"].as_str().unwrap_or("manual") {
        "time" => Trigger::Time {
            time: s("time"),
            days: v["days"]
                .as_array()
                .map(|d| {
                    d.iter()
                        .filter_map(|x| x.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default(),
        },
        "notification" => Trigger::Notification {
            app: s("app"),
            contains: s("contains"),
        },
        "download" => Trigger::Download { kind: s("kind") },
        "meeting_ended" => Trigger::MeetingEnded,
        "app_opened" => Trigger::AppOpened { app: s("app") },
        _ => Trigger::Manual,
    }
}

pub fn describe_trigger(t: &Trigger) -> String {
    match t {
        Trigger::Manual => "when asked".into(),
        Trigger::Time { time, days } if days.is_empty() => format!("every day at {time}"),
        Trigger::Time { time, days } => format!("{} at {time}", days.join(", ")),
        Trigger::Notification { app, contains } => format!(
            "on a notification{}{}",
            if app.is_empty() {
                String::new()
            } else {
                format!(" from {app}")
            },
            if contains.is_empty() {
                String::new()
            } else {
                format!(" with \"{contains}\"")
            }
        ),
        Trigger::Download { kind } if kind.is_empty() => "when a download finishes".into(),
        Trigger::Download { kind } => format!("when a {kind} download finishes"),
        Trigger::MeetingEnded => "when a meeting ends".into(),
        Trigger::AppOpened { app } => format!("when {app} opens (once a day)"),
    }
}

/// The `recipes` tool for models.
pub fn tool(app: &AppHandle, args: &Value) -> String {
    let name = args["name"].as_str().unwrap_or_default();
    let out = match args["action"].as_str().unwrap_or("list") {
        "list" => {
            let all = recipes(app);
            Ok(if all.is_empty() {
                "No recipes yet.".into()
            } else {
                all.iter()
                    .map(|r| {
                        format!(
                            "- {} ({}{}): {}",
                            r.name,
                            describe_trigger(&r.trigger),
                            if r.enabled { "" } else { ", off" },
                            r.prompt
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
        }
        "create" => save(
            app,
            Recipe {
                name: name.to_owned(),
                prompt: args["prompt"].as_str().unwrap_or_default().to_owned(),
                trigger: trigger_from(args),
                // Starting alone is the user's choice, in Settings.
                auto: false,
                ..Default::default()
            },
        ),
        "run" => run_by_id(app, name),
        "delete" => delete(app, name),
        other => Err(format!("unknown recipes action {other}")),
    };
    out.unwrap_or_else(|e| format!("Error: {e}"))
}

/// Things the user asked Sidekick to remember.
pub fn remember(app: &AppHandle, fact: &str, forget: bool) -> Result<String, String> {
    let fact = fact.trim();
    if fact.is_empty() {
        return Err("remember what?".into());
    }
    let mut next = lock(&app.state::<AppState>().settings).clone();
    if forget {
        let lower = fact.to_lowercase();
        next.memory.retain(|m| !m.to_lowercase().contains(&lower));
    } else if !next.memory.iter().any(|m| m.eq_ignore_ascii_case(fact)) {
        next.memory.push(fact.chars().take(200).collect());
        if next.memory.len() > 50 {
            next.memory.remove(0);
        }
    }
    crate::commands::apply_settings(app, next)?;
    Ok(if forget {
        "Forgotten".into()
    } else {
        format!("Remembered: {fact}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_triggers() {
        assert!(time_due("17:00", &["fri".into()], "17:00", "fri"));
        assert!(time_due("9:5", &[], "09:05", "mon"));
        assert!(time_due("09:00", &["Friday".into()], "09:00", "fri"));
        assert!(!time_due("17:00", &["fri".into()], "17:00", "thu"));
        assert!(!time_due("17:00", &[], "17:01", "mon"));
    }

    #[test]
    fn event_triggers() {
        let pdf = json!({ "name": "invoice.pdf", "ext": "pdf", "kind": "document" });
        assert!(
            event_matches(
                &Trigger::Download { kind: "pdf".into() },
                "file.download_completed",
                &pdf
            )
            .is_some()
        );
        assert!(
            event_matches(
                &Trigger::Download {
                    kind: "image".into()
                },
                "file.download_completed",
                &pdf
            )
            .is_none()
        );
        assert!(
            event_matches(
                &Trigger::MeetingEnded,
                "calendar.meeting_ended",
                &json!({ "title": "Standup" })
            )
            .unwrap()
            .contains("Standup")
        );
        assert!(
            event_matches(
                &Trigger::AppOpened {
                    app: "slack".into()
                },
                "window.focused",
                &json!({ "app": "Slack.exe" })
            )
            .is_some()
        );
        assert!(event_matches(&Trigger::Manual, "window.focused", &json!({})).is_none());
        let t = Trigger::Notification {
            app: "WhatsApp".into(),
            contains: "invoice".into(),
        };
        assert!(notification_matches(
            &t,
            "WhatsApp",
            "Ali",
            "Send the invoice"
        ));
        assert!(!notification_matches(
            &t,
            "Slack",
            "Ali",
            "Send the invoice"
        ));
    }

    #[test]
    fn names_and_triggers_from_models() {
        assert_eq!(
            short_name("send my timesheet to Sara every Friday please."),
            "Send my timesheet to Sara every"
        );
        assert_eq!(normalize("  Run: Send timesheet! "), "run send timesheet");
        let t = trigger_from(&json!({ "when": "time", "time": "17:00", "days": ["fri"] }));
        assert_eq!(describe_trigger(&t), "fri at 17:00");
        assert_eq!(trigger_from(&json!({})), Trigger::Manual);
    }
}
