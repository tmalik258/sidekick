//! Fills the calendar sensor from Google Calendar and Outlook through
//! Composio, every few minutes and right after an app is connected.

use std::time::Duration;

use chrono::Utc;
use serde_json::json;
use sidekick_sensors::calendar::{Meeting, from_google, from_outlook};
use tauri::{AppHandle, Listener, Manager};

use crate::composio_api::find_array;
use crate::state::{AppState, lock};

const EVERY: Duration = Duration::from_secs(10 * 60);
const FIRST_AFTER: Duration = Duration::from_secs(20);

pub fn start(app: &AppHandle) {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<()>();
    app.listen(crate::composio::CHANGED_EVENT, move |_| {
        let _ = tx.send(());
    });
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_AFTER).await;
        loop {
            if !crate::state::is_paused(&app) {
                refresh(&app).await;
            }
            tokio::select! {
                _ = tokio::time::sleep(EVERY) => {}
                _ = rx.recv() => {}
            }
        }
    });
}

pub async fn refresh(app: &AppHandle) {
    let settings = lock(&app.state::<AppState>().settings).clone();
    let wanted = settings.sensors.get("calendar") != Some(&false);
    let mut meetings: Vec<Meeting> = Vec::new();
    let mut sources = Vec::new();
    let mut errors = Vec::new();
    if wanted && crate::composio::signed_in() && settings.composio.enabled {
        match crate::composio::apps(&settings.composio).await {
            Ok(apps) => {
                let has = |slug: &str| apps.iter().any(|a| a.slug == slug && a.connected);
                let now = Utc::now();
                let from = (now - chrono::Duration::hours(2)).to_rfc3339();
                let to = (now + chrono::Duration::hours(30)).to_rfc3339();
                if has("googlecalendar") {
                    let args = json!({
                        "calendarId": "primary", "timeMin": from, "timeMax": to,
                        "singleEvents": true, "orderBy": "startTime", "maxResults": 50,
                    });
                    match crate::composio::run_tool(
                        &settings.composio,
                        "GOOGLECALENDAR_EVENTS_LIST",
                        args,
                    )
                    .await
                    {
                        Ok(data) => {
                            sources.push("Google Calendar".to_owned());
                            if let Some(items) = find_array(&data, &["items"]) {
                                meetings.extend(items.iter().filter_map(from_google));
                            }
                        }
                        Err(e) => errors.push(e),
                    }
                }
                if has("outlook") {
                    let args = json!({
                        "start_datetime": from, "end_datetime": to, "timezone": "UTC", "top": 50,
                    });
                    match crate::composio::run_tool(
                        &settings.composio,
                        "OUTLOOK_GET_CALENDAR_VIEW",
                        args,
                    )
                    .await
                    {
                        Ok(data) => {
                            sources.push("Outlook".to_owned());
                            if let Some(items) = find_array(&data, &["value", "events", "items"]) {
                                meetings.extend(items.iter().filter_map(from_outlook));
                            }
                        }
                        Err(e) => errors.push(e),
                    }
                }
            }
            Err(e) => errors.push(e),
        }
    }
    meetings.sort_by_key(|m| m.start);
    meetings.dedup_by(|a, b| a.uid == b.uid && a.start == b.start);
    let state = app.state::<AppState>();
    let mut c = state
        .calendar
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    c.meetings = meetings;
    c.sources = sources;
    c.error = (!errors.is_empty()).then(|| errors.join("; "));
}
