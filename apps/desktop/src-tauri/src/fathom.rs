//! Fathom follow-up: after a meeting, fetch its Fathom summary
//! and action items through Composio (Fathom connected there) and have the
//! AI draft the follow-up in Ask mode.

use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use tauri::{AppHandle, Manager};

use crate::state::{AppState, lock};

#[derive(Debug, Clone, PartialEq)]
pub struct Notes {
    pub recording_id: Option<i64>,
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
    let items = crate::composio_api::find_array(body, &["items", "meetings"])?;
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
        recording_id: item["recording_id"]
            .as_i64()
            .or_else(|| item["recording_id"].as_str().and_then(|s| s.parse().ok())),
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

/// The first string under `key`, searched depth first.
fn find_str<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    match v {
        Value::Object(map) => map
            .get(key)
            .and_then(Value::as_str)
            .or_else(|| map.values().find_map(|x| find_str(x, key))),
        Value::Array(items) => items.iter().find_map(|x| find_str(x, key)),
        _ => None,
    }
}

pub async fn follow_up(app: &AppHandle, start: &str, title: &str) -> Result<String, String> {
    let composio = lock(&app.state::<AppState>().settings).composio.clone();
    let start = DateTime::parse_from_rfc3339(start)
        .ok()
        .map(|t| t.with_timezone(&Utc));
    let mut args = json!({ "include_action_items": true });
    if let Some(s) = start {
        args["created_after"] = json!((s - chrono::Duration::hours(2)).to_rfc3339());
    }
    let body = crate::composio::run_tool(&composio, "FATHOM_LIST_MEETINGS", args).await?;
    let mut notes =
        pick(&body, start).ok_or(format!("No Fathom recording found for {title} yet."))?;
    if notes.summary.is_empty()
        && let Some(id) = notes.recording_id
    {
        let summary = crate::composio::run_tool(
            &composio,
            "FATHOM_GET_RECORDING_SUMMARY",
            json!({ "recording_id": id }),
        )
        .await?;
        notes.summary = find_str(&summary, "markdown_formatted")
            .or_else(|| find_str(&summary, "summary"))
            .unwrap_or_default()
            .trim()
            .to_owned();
    }
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
    // Meeting notes become searchable in Ask ("what did we decide on X").
    crate::search::put(
        app,
        "meeting",
        notes
            .share_url
            .as_deref()
            .unwrap_or(&format!("meeting:{}", notes.title)),
        &notes.title,
        &page,
        &chrono::Utc::now().to_rfc3339(),
    );
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
    fn finds_summary_text_anywhere() {
        let v = json!({ "data": { "summary": { "markdown_formatted": "## Notes" } } });
        assert_eq!(find_str(&v, "markdown_formatted"), Some("## Notes"));
    }

    #[test]
    fn picks_the_meeting_by_start_time() {
        let body = serde_json::json!({ "data": {
            "items": [
                {
                    "title": "Weekly sync",
                    "scheduled_start_time": "2026-10-01T09:00:00Z",
                    "default_summary": { "markdown_formatted": "## Summary\nOld" }
                },
                {
                    "recording_id": 42,
                    "title": "Design review",
                    "share_url": "https://fathom.video/share/abc",
                    "scheduled_start_time": "2026-10-01T15:00:00Z",
                    "default_summary": { "template_name": "general", "markdown_formatted": "## Summary\nAgreed on mocks." },
                    "action_items": [{ "description": "Sara sends mocks" }]
                }
            ],
            "next_cursor": null
        }});
        let start = DateTime::parse_from_rfc3339("2026-10-01T15:02:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let n = pick(&body, Some(start)).unwrap();
        assert_eq!(n.title, "Design review");
        assert_eq!(n.recording_id, Some(42));
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
