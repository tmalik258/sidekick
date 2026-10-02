//! Calendar (FR-COMM-02): meetings come from Google Calendar or Outlook
//! through Composio (the app fills [`CalendarState::meetings`]). Raises
//! `calendar.meeting_soon` a few minutes before a meeting and
//! `calendar.meeting_ended` after it.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Local, NaiveDate, TimeZone, Utc};
use serde::Serialize;
use serde_json::Value;
use sidekick_core::{Event, EventBus, Sensitivity};
use tokio::task::JoinHandle;

use crate::{Sensor, SensorGate};

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Meeting {
    pub uid: String,
    pub title: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub location: String,
    pub description: String,
    /// A video call link found in the event, if any.
    pub join_url: Option<String>,
    pub attendees: Vec<String>,
}

/// Shared between the sensor and the app: the meetings in the next day,
/// and where they came from.
#[derive(Debug, Default)]
pub struct CalendarState {
    pub remind_minutes: i64,
    pub meetings: Vec<Meeting>,
    /// Calendars read, for Settings ("Google Calendar").
    pub sources: Vec<String>,
    pub error: Option<String>,
}

pub type Calendar = Arc<Mutex<CalendarState>>;

pub struct CalendarSensor {
    pub state: Calendar,
}

impl CalendarSensor {
    pub const ID: &'static str = "calendar";
    pub const SOON: &'static str = "calendar.meeting_soon";
    pub const ENDED: &'static str = "calendar.meeting_ended";
}

const TICK: Duration = Duration::from_secs(30);
/// Meeting-ended follow-ups wait for recordings and notes to be processed.
const ENDED_AFTER_MINUTES: i64 = 5;
const MAX_DESCRIPTION: usize = 2000;

impl Sensor for CalendarSensor {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn spawn(self: Box<Self>, bus: EventBus, gate: SensorGate) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut announced: HashSet<String> = HashSet::new();
            let mut tick = tokio::time::interval(TICK);
            loop {
                tick.tick().await;
                if !gate.allows(Self::ID) {
                    continue;
                }
                let (meetings, remind) = {
                    let s = lock(&self.state);
                    (s.meetings.clone(), s.remind_minutes.max(1))
                };
                for event in due(&meetings, remind, Utc::now(), &mut announced) {
                    bus.publish(event);
                }
                if announced.len() > 500 {
                    announced.clear();
                }
            }
        })
    }
}

/// The reminders and follow-ups due now; each is raised once.
fn due(
    meetings: &[Meeting],
    remind: i64,
    now: DateTime<Utc>,
    announced: &mut HashSet<String>,
) -> Vec<Event> {
    let mut out = Vec::new();
    for m in meetings {
        let until = (m.start - now).num_minutes();
        let key = format!("soon:{}:{}", m.uid, m.start.timestamp());
        if m.start > now && until < remind && announced.insert(key) {
            out.push(soon_event(m, until + 1));
        }
        let since_end = (now - m.end).num_minutes();
        let key = format!("ended:{}:{}", m.uid, m.start.timestamp());
        if (ENDED_AFTER_MINUTES..ENDED_AFTER_MINUTES + 10).contains(&since_end)
            && (m.end - m.start).num_minutes() >= 10
            && announced.insert(key)
        {
            out.push(ended_event(m));
        }
    }
    out
}

fn lock(state: &Calendar) -> std::sync::MutexGuard<'_, CalendarState> {
    state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn local_time(t: DateTime<Utc>) -> String {
    t.with_timezone(&Local).format("%H:%M").to_string()
}

fn soon_event(m: &Meeting, minutes: i64) -> Event {
    let description: String = m.description.chars().take(MAX_DESCRIPTION).collect();
    Event::new(
        CalendarSensor::SOON,
        CalendarSensor::ID,
        serde_json::json!({
            "title": m.title,
            "minutes": minutes,
            "start": local_time(m.start),
            "join_url": m.join_url.clone().unwrap_or_default(),
            "location": m.location,
            "attendees": m.attendees.join(", "),
            "details": format!(
                "Meeting: {}\nStarts: {}\nAttendees: {}\nLocation: {}\n\n{}",
                m.title, local_time(m.start), m.attendees.join(", "), m.location, description
            ),
        }),
    )
    .with_sensitivity(Sensitivity::Personal)
}

fn ended_event(m: &Meeting) -> Event {
    Event::new(
        CalendarSensor::ENDED,
        CalendarSensor::ID,
        serde_json::json!({
            "title": m.title,
            "start": local_time(m.start),
            "start_utc": m.start.to_rfc3339(),
            "attendees": m.attendees.join(", "),
        }),
    )
    .with_sensitivity(Sensitivity::Personal)
}

const CALL_HOSTS: &[&str] = &[
    "https://meet.google.com/",
    "https://zoom.us/",
    "https://us02web.zoom.us/",
    "https://us04web.zoom.us/",
    "https://us05web.zoom.us/",
    "https://us06web.zoom.us/",
    "https://teams.microsoft.com/",
    "https://teams.live.com/",
    "https://whereby.com/",
    "https://app.gather.town/",
    "https://meet.jit.si/",
];

/// The first video call link in the given texts.
pub fn find_join_url(texts: &[&str]) -> Option<String> {
    for text in texts {
        for word in text
            .split(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '(' | ')' | ','))
        {
            let word = word.trim_end_matches(['.', ';']);
            if CALL_HOSTS.iter().any(|h| word.starts_with(h))
                || (word.starts_with("https://") && word.contains(".zoom.us/j/"))
            {
                return Some(word.to_owned());
            }
        }
    }
    None
}

fn rfc3339(v: &Value) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(v.as_str()?)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

/// A Google Calendar event (`GOOGLECALENDAR_EVENTS_LIST`). All-day events
/// (with only a `date`) and cancelled or declined ones are skipped.
pub fn from_google(e: &Value) -> Option<Meeting> {
    if e["status"].as_str() == Some("cancelled") {
        return None;
    }
    let declined = e["attendees"].as_array().is_some_and(|a| {
        a.iter()
            .any(|p| p["self"].as_bool() == Some(true) && p["responseStatus"] == "declined")
    });
    if declined {
        return None;
    }
    let start = rfc3339(&e["start"]["dateTime"])?;
    let end = rfc3339(&e["end"]["dateTime"]).unwrap_or(start + chrono::Duration::minutes(30));
    let description = e["description"].as_str().unwrap_or_default();
    let location = e["location"].as_str().unwrap_or_default();
    let entry_points: Vec<&str> = e["conferenceData"]["entryPoints"]
        .as_array()
        .map(|a| a.iter().filter_map(|p| p["uri"].as_str()).collect())
        .unwrap_or_default();
    let mut texts: Vec<&str> = vec![e["hangoutLink"].as_str().unwrap_or_default()];
    texts.extend(entry_points);
    texts.extend([location, description]);
    Some(Meeting {
        uid: e["id"].as_str().unwrap_or_default().to_owned(),
        title: e["summary"].as_str().unwrap_or("Meeting").to_owned(),
        start,
        end,
        location: location.to_owned(),
        description: description.to_owned(),
        join_url: find_join_url(&texts),
        attendees: e["attendees"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter(|p| p["self"].as_bool() != Some(true))
                    .filter_map(|p| p["displayName"].as_str().or(p["email"].as_str()))
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
    })
}

/// Outlook times come as `{dateTime, timeZone}`; Sidekick asks for UTC.
fn outlook_time(v: &Value) -> Option<DateTime<Utc>> {
    let s = v["dateTime"].as_str()?;
    if let Ok(t) = DateTime::parse_from_rfc3339(s) {
        return Some(t.with_timezone(&Utc));
    }
    let naive =
        chrono::NaiveDateTime::parse_from_str(s.trim_end_matches('Z'), "%Y-%m-%dT%H:%M:%S%.f")
            .ok()?;
    Some(Utc.from_utc_datetime(&naive))
}

/// An Outlook event (`OUTLOOK_GET_CALENDAR_VIEW`, times in UTC).
pub fn from_outlook(e: &Value) -> Option<Meeting> {
    if e["isCancelled"].as_bool() == Some(true) || e["isAllDay"].as_bool() == Some(true) {
        return None;
    }
    let start = outlook_time(&e["start"])?;
    let end = outlook_time(&e["end"]).unwrap_or(start + chrono::Duration::minutes(30));
    let location = e["location"]["displayName"].as_str().unwrap_or_default();
    let description = e["bodyPreview"].as_str().unwrap_or_default();
    let texts = [
        e["onlineMeeting"]["joinUrl"].as_str().unwrap_or_default(),
        location,
        description,
    ];
    Some(Meeting {
        uid: e["id"].as_str().unwrap_or_default().to_owned(),
        title: e["subject"].as_str().unwrap_or("Meeting").to_owned(),
        start,
        end,
        location: location.to_owned(),
        description: description.to_owned(),
        join_url: find_join_url(&texts),
        attendees: e["attendees"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|p| {
                        p["emailAddress"]["name"]
                            .as_str()
                            .or(p["emailAddress"]["address"].as_str())
                    })
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
    })
}

/// Meetings on a local calendar day.
pub fn on_day(meetings: &[Meeting], day: NaiveDate) -> Vec<Meeting> {
    meetings
        .iter()
        .filter(|m| m.start.with_timezone(&Local).date_naive() == day)
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn at(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn reads_google_events() {
        let e = json!({
            "id": "e1", "status": "confirmed", "summary": "Design review",
            "start": { "dateTime": "2026-10-02T15:00:00+05:00" },
            "end": { "dateTime": "2026-10-02T15:30:00+05:00" },
            "hangoutLink": "https://meet.google.com/abc-defg-hij",
            "attendees": [
                { "email": "me@x.com", "self": true, "responseStatus": "accepted" },
                { "email": "sara@client.com", "displayName": "Sara" }
            ]
        });
        let m = from_google(&e).unwrap();
        assert_eq!(m.title, "Design review");
        assert_eq!(m.start, at("2026-10-02T10:00:00Z"));
        assert_eq!(
            m.join_url.as_deref(),
            Some("https://meet.google.com/abc-defg-hij")
        );
        assert_eq!(m.attendees, vec!["Sara"]);

        let all_day =
            json!({ "id": "e2", "summary": "Holiday", "start": { "date": "2026-10-02" } });
        assert!(from_google(&all_day).is_none());
        let mut declined = e.clone();
        declined["attendees"][0]["responseStatus"] = json!("declined");
        assert!(from_google(&declined).is_none());
        let mut cancelled = e;
        cancelled["status"] = json!("cancelled");
        assert!(from_google(&cancelled).is_none());
    }

    #[test]
    fn reads_outlook_events() {
        let e = json!({
            "id": "o1", "subject": "Standup", "isAllDay": false, "isCancelled": false,
            "start": { "dateTime": "2026-10-02T09:00:00.0000000", "timeZone": "UTC" },
            "end": { "dateTime": "2026-10-02T09:15:00.0000000", "timeZone": "UTC" },
            "onlineMeeting": { "joinUrl": "https://teams.microsoft.com/l/meetup-join/x" },
            "location": { "displayName": "Teams" },
            "attendees": [{ "emailAddress": { "name": "Ali", "address": "ali@x.com" } }]
        });
        let m = from_outlook(&e).unwrap();
        assert_eq!(m.start, at("2026-10-02T09:00:00Z"));
        assert_eq!(m.end, at("2026-10-02T09:15:00Z"));
        assert!(
            m.join_url
                .unwrap()
                .starts_with("https://teams.microsoft.com/")
        );
        assert_eq!(m.attendees, vec!["Ali"]);
        let mut cancelled = e;
        cancelled["isCancelled"] = json!(true);
        assert!(from_outlook(&cancelled).is_none());
    }

    #[test]
    fn reminds_once_before_and_follows_up_once_after() {
        let m = Meeting {
            uid: "u".into(),
            title: "Sync".into(),
            start: at("2026-10-02T10:00:00Z"),
            end: at("2026-10-02T10:30:00Z"),
            location: String::new(),
            description: String::new(),
            join_url: None,
            attendees: vec![],
        };
        let mut seen = HashSet::new();
        let ms = std::slice::from_ref(&m);
        assert!(due(ms, 5, at("2026-10-02T09:50:00Z"), &mut seen).is_empty());
        let soon = due(ms, 5, at("2026-10-02T09:56:00Z"), &mut seen);
        assert_eq!(soon.len(), 1);
        assert_eq!(soon[0].kind, CalendarSensor::SOON);
        assert!(due(ms, 5, at("2026-10-02T09:57:00Z"), &mut seen).is_empty());
        let ended = due(ms, 5, at("2026-10-02T10:36:00Z"), &mut seen);
        assert_eq!(ended.len(), 1);
        assert_eq!(ended[0].kind, CalendarSensor::ENDED);
    }

    #[test]
    fn finds_call_links_only_on_known_hosts() {
        assert_eq!(
            find_join_url(&["Join: https://us05web.zoom.us/j/123?pwd=x."]).as_deref(),
            Some("https://us05web.zoom.us/j/123?pwd=x")
        );
        assert!(find_join_url(&["https://evil.example/meet.google.com/"]).is_none());
    }
}
