//! Drops old Ask chats that are not worth keeping. SemIf (or the local
//! model) judges each chat older than a day; without a decider, short
//! chats are dropped and longer ones kept.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use chrono::{Duration as ChronoDuration, Utc};
use sidekick_ai::{Decider, Decision, DecisionOption};
use sidekick_sensors::classify::{ClipKind, clip_kind};
use tauri::{AppHandle, Manager};

use crate::ai;
use crate::state::{AppState, lock};

const MAX_AGE: ChronoDuration = ChronoDuration::hours(24);
const BATCH: u32 = 10;
const EXCERPT: usize = 200;

/// Runs one prune pass: age gate, then SemIf keep/drop (or the heuristic).
pub async fn run(app: &AppHandle) {
    // While Ask is open the user may be reading an old chat; wait for the
    // next tick so we do not delete under them.
    if app.state::<AppState>().ask_open.load(Ordering::SeqCst) {
        return;
    }
    let cutoff = (Utc::now() - MAX_AGE).to_rfc3339();
    let storage = app.state::<AppState>().storage.clone();
    let rows = match tauri::async_runtime::spawn_blocking({
        let storage = storage.clone();
        let cutoff = cutoff.clone();
        move || lock(&storage).chats_older_than(&cutoff, BATCH)
    })
    .await
    {
        Ok(Ok(rows)) => rows,
        Ok(Err(err)) => {
            log::warn!("could not list old chats: {err}");
            return;
        }
        Err(err) => {
            log::warn!("chat prune task failed: {err}");
            return;
        }
    };
    if rows.is_empty() {
        return;
    }

    let settings = lock(&app.state::<AppState>().settings).clone();
    let deciders = ai::deciders(app, &settings);
    let mut dropped = 0u32;
    let mut kept = 0u32;

    for row in rows {
        let (user_turns, excerpt) = user_turns_and_excerpt(&row.turns_json);
        let keep = if deciders.is_empty() {
            keep_without_model(user_turns)
        } else {
            match judge(&deciders, &row.id, &row.title, &row.updated, user_turns, &excerpt).await {
                Some(keep) => keep,
                None => keep_without_model(user_turns),
            }
        };
        if keep {
            kept += 1;
            continue;
        }
        let id = row.id.clone();
        let deleted = tauri::async_runtime::spawn_blocking({
            let storage = storage.clone();
            move || lock(&storage).delete_chat(&id)
        })
        .await;
        match deleted {
            Ok(Ok(())) => {
                dropped += 1;
                log::info!("dropped old Ask chat {}", row.id);
            }
            Ok(Err(err)) => log::warn!("could not delete chat {}: {err}", row.id),
            Err(err) => log::warn!("delete chat task failed: {err}"),
        }
    }

    if dropped > 0 || kept > 0 {
        log::info!("chat prune: kept {kept}, dropped {dropped}");
    }
}

/// Without SemIf or a local model: keep chats with at least two user turns.
pub fn keep_without_model(user_turns: usize) -> bool {
    user_turns >= 2
}

fn user_turns_and_excerpt(turns_json: &str) -> (usize, String) {
    let Ok(turns) = serde_json::from_str::<Vec<serde_json::Value>>(turns_json) else {
        return (0, String::new());
    };
    let users: Vec<&str> = turns
        .iter()
        .filter(|t| t.get("role").and_then(|r| r.as_str()) == Some("user"))
        .filter_map(|t| t.get("content").and_then(|c| c.as_str()))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    let first = users.first().copied().unwrap_or("");
    let excerpt = if first.is_empty() {
        String::new()
    } else if clip_kind(first) == ClipKind::Secret {
        "(redacted)".into()
    } else {
        first.chars().take(EXCERPT).collect()
    };
    (users.len(), excerpt)
}

fn decision(
    id: &str,
    title: &str,
    updated: &str,
    user_turns: usize,
    excerpt: &str,
) -> Decision {
    let age = ago_label(updated);
    let clip = if excerpt.is_empty() {
        "No user message saved.".to_owned()
    } else {
        format!("First question: {excerpt}")
    };
    Decision {
        id: id.to_owned(),
        state: format!(
            "Past Ask chat titled \"{title}\". Last updated {age}. {user_turns} user message(s). {clip}"
        ),
        question: "Should Sidekick keep this past Ask chat?".into(),
        options: vec![
            DecisionOption {
                id: "keep".into(),
                description: "Keep it; useful to continue later".into(),
            },
            DecisionOption {
                id: "drop".into(),
                description: "Drop it; one-off, empty, or no longer useful".into(),
            },
        ],
    }
}

async fn judge(
    deciders: &[Arc<dyn Decider>],
    id: &str,
    title: &str,
    updated: &str,
    user_turns: usize,
    excerpt: &str,
) -> Option<bool> {
    let d = decision(id, title, updated, user_turns, excerpt);
    for decider in deciders {
        if !decider.available().await {
            continue;
        }
        match decider.decide(&d).await {
            Ok(ranked) => {
                let (picked, score) = ranked.best()?;
                log::info!(
                    "{} picked {picked} ({score:.2}) for chat prune {id}",
                    decider.id()
                );
                return Some(picked == "keep");
            }
            Err(err) => log::warn!("{} could not judge chat {id}: {err}", decider.id()),
        }
    }
    None
}

fn ago_label(iso: &str) -> String {
    let Ok(then) = chrono::DateTime::parse_from_rfc3339(iso) else {
        return iso.to_owned();
    };
    let hours = (Utc::now() - then.with_timezone(&Utc))
        .num_hours()
        .max(0);
    if hours < 48 {
        format!("{hours} hours ago")
    } else {
        format!("{} days ago", hours / 24)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heuristic_keeps_multi_turn_only() {
        assert!(!keep_without_model(0));
        assert!(!keep_without_model(1));
        assert!(keep_without_model(2));
        assert!(keep_without_model(5));
    }

    #[test]
    fn counts_user_turns_and_redacts_secrets() {
        let (n, ex) = user_turns_and_excerpt(
            r#"[{"role":"user","content":"API_KEY=supersecretvalue"},{"role":"assistant","content":"ok"},{"role":"user","content":"thanks"}]"#,
        );
        assert_eq!(n, 2);
        assert_eq!(ex, "(redacted)");

        let (n2, ex2) = user_turns_and_excerpt(
            r#"[{"role":"user","content":"What is on my calendar?"}]"#,
        );
        assert_eq!(n2, 1);
        assert!(ex2.contains("calendar"));
    }
}
