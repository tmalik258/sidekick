//! Fathom follow-up (FR-COMM-03): after a meeting, fetch its Fathom summary
//! and action items with your own API key (FATHOM_API_KEY, never stored)
//! and have the AI draft the follow-up in Ask mode.

use std::time::Duration;

use chrono::{DateTime, Utc};
use serde_json::Value;
use tauri::AppHandle;

const API: &str = "https://api.fathom.ai/external/v1/meetings";

#[derive(Debug, Clone, PartialEq)]
pub struct Notes {
    pub title: String,
    pub summary: String,
    pub action_items: Vec<String>,
    pub share_url: Option<String>,
}

fn time(v: &Value) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(v.as_str()?)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

/// Picks the meeting that started closest to `start` (within two hours), or
/// the newest one when the start is unknown.
pub fn pick(body: &Value, start: Option<DateTime<Utc>>) -> Option<Notes> {
    let items = body["items"].as_array()?;
    let started =
        |m: &Value| time(&m["scheduled_start_time"]).or_else(|| time(&m["recording_start_time"]));
    let item = match start {
        Some(s) => items
            .iter()
            .filter_map(|m| started(m).map(|t| ((t - s).num_minutes().abs(), m)))
            .filter(|(d, _)| *d <= 120)
            .min_by_key(|(d, _)| *d)
            .map(|(_, m)| m),
        None => items.first(),
    }?;
    let summary = item["default_summary"]["markdown_formatted"]
        .as_str()
        .unwrap_or_default()
        .trim()
        .to_owned();
    let action_items = item["action_items"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|i| i["description"].as_str().or(i.as_str()))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    Some(Notes {
        title: item["title"]
            .as_str()
            .or(item["meeting_title"].as_str())
            .unwrap_or("Meeting")
            .to_owned(),
        summary,
        action_items,
        share_url: item["share_url"]
            .as_str()
            .filter(|u| u.starts_with("https://"))
            .map(str::to_owned),
    })
}

async fn fetch(key: &str, start: Option<DateTime<Utc>>) -> Result<Value, String> {
    let client = reqwest::Client::builder()
        .user_agent("Sidekick")
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;
    let mut req = client.get(API).header("X-Api-Key", key).query(&[
        ("include_summary", "true"),
        ("include_action_items", "true"),
    ]);
    if let Some(s) = start {
        req = req.query(&[(
            "created_after",
            (s - chrono::Duration::hours(2)).to_rfc3339(),
        )]);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| format!("Fathom: {}", e.without_url()))?;
    match resp.status().as_u16() {
        200 => resp.json().await.map_err(|e| format!("Fathom: {e}")),
        401 | 403 => Err("Fathom did not accept FATHOM_API_KEY.".into()),
        429 => Err("Fathom is rate limiting; try again in a minute.".into()),
        s => Err(format!("Fathom answered {s}.")),
    }
}

pub async fn follow_up(app: &AppHandle, start: &str, title: &str) -> Result<String, String> {
    let key = std::env::var("FATHOM_API_KEY")
        .ok()
        .filter(|k| !k.trim().is_empty())
        .ok_or("Set FATHOM_API_KEY to use Fathom notes.")?;
    let start = DateTime::parse_from_rfc3339(start)
        .ok()
        .map(|t| t.with_timezone(&Utc));
    let body = fetch(key.trim(), start).await?;
    let notes = pick(&body, start).ok_or(format!("No Fathom recording found for {title} yet."))?;
    if notes.summary.is_empty() {
        return Err("Fathom is still writing the notes; try again in a few minutes.".into());
    }
    let mut page = format!("Meeting: {}\n\n{}", notes.title, notes.summary);
    if !notes.action_items.is_empty() {
        page.push_str("\n\nAction items:\n");
        for item in &notes.action_items {
            page.push_str(&format!("- {item}\n"));
        }
    }
    if let Some(url) = &notes.share_url {
        page.push_str(&format!("\nRecording: {url}\n"));
    }
    crate::ask::open(
        app,
        crate::ask::Open {
            prompt: Some(
                "Draft a short, friendly follow-up message for the attendees of this meeting: thanks, the decisions, and the action items with owners. Include the recording link if there is one.".into(),
            ),
            ask: true,
            page: Some(page),
            ..Default::default()
        },
    );
    Ok(format!("Fathom notes for {}", notes.title))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_meeting_by_start_time() {
        let body = serde_json::json!({
            "items": [
                {
                    "title": "Weekly sync",
                    "scheduled_start_time": "2026-10-01T09:00:00Z",
                    "default_summary": { "markdown_formatted": "## Summary\nOld" }
                },
                {
                    "title": "Design review",
                    "share_url": "https://fathom.video/share/abc",
                    "scheduled_start_time": "2026-10-01T15:00:00Z",
                    "default_summary": { "template_name": "general", "markdown_formatted": "## Summary\nAgreed on mocks." },
                    "action_items": [{ "description": "Sara sends mocks" }]
                }
            ],
            "next_cursor": null
        });
        let start = DateTime::parse_from_rfc3339("2026-10-01T15:02:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let n = pick(&body, Some(start)).unwrap();
        assert_eq!(n.title, "Design review");
        assert_eq!(n.summary, "## Summary\nAgreed on mocks.");
        assert_eq!(n.action_items, vec!["Sara sends mocks"]);
        assert_eq!(
            n.share_url.as_deref(),
            Some("https://fathom.video/share/abc")
        );
        let far = DateTime::parse_from_rfc3339("2026-10-03T15:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(pick(&body, Some(far)).is_none());
        assert_eq!(pick(&body, None).unwrap().title, "Weekly sync");
    }
}
