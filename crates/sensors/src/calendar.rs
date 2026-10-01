//! Calendar (FR-COMM-02): reads your calendar's private iCal link (Google
//! Calendar "Secret address in iCal format", or an Outlook published ICS
//! link), so there is no account sign-in. Raises `calendar.meeting_soon` a
//! few minutes before a meeting and `calendar.meeting_ended` after it.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, TimeZone, Utc};
use serde::Serialize;
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

/// Shared between the sensor and the app: the feeds to read and the
/// meetings found in the next day.
#[derive(Debug, Default)]
pub struct CalendarState {
    pub feeds: Vec<String>,
    pub remind_minutes: i64,
    pub meetings: Vec<Meeting>,
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

const FETCH_EVERY: Duration = Duration::from_secs(10 * 60);
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
            let client = reqwest::Client::builder()
                .user_agent("Sidekick")
                .timeout(Duration::from_secs(30))
                .build()
                .unwrap_or_default();
            let mut last_fetch: Option<tokio::time::Instant> = None;
            let mut last_feeds: Vec<String> = Vec::new();
            let mut announced: HashSet<String> = HashSet::new();
            let mut tick = tokio::time::interval(TICK);
            loop {
                tick.tick().await;
                if !gate.allows(Self::ID) {
                    continue;
                }
                let feeds = lock(&self.state).feeds.clone();
                if feeds.is_empty() {
                    lock(&self.state).meetings.clear();
                    continue;
                }
                let stale =
                    last_fetch.is_none_or(|t| t.elapsed() >= FETCH_EVERY) || feeds != last_feeds;
                if stale {
                    last_fetch = Some(tokio::time::Instant::now());
                    last_feeds = feeds.clone();
                    let now = Utc::now();
                    let mut meetings = Vec::new();
                    let mut errors = Vec::new();
                    for url in &feeds {
                        match fetch(&client, url).await {
                            Ok(text) => meetings.extend(parse(
                                &text,
                                now - chrono::Duration::hours(2),
                                now + chrono::Duration::hours(30),
                            )),
                            Err(e) => errors.push(e),
                        }
                    }
                    meetings.sort_by_key(|m| m.start);
                    meetings.dedup_by(|a, b| a.uid == b.uid && a.start == b.start);
                    let mut s = lock(&self.state);
                    s.meetings = meetings;
                    s.error = (!errors.is_empty()).then(|| errors.join("; "));
                }
                let (meetings, remind) = {
                    let s = lock(&self.state);
                    (s.meetings.clone(), s.remind_minutes.max(1))
                };
                let now = Utc::now();
                for m in &meetings {
                    let until = (m.start - now).num_minutes();
                    let key = format!("soon:{}:{}", m.uid, m.start.timestamp());
                    if m.start > now && until < remind && announced.insert(key) {
                        bus.publish(soon_event(m, until + 1));
                    }
                    let since_end = (now - m.end).num_minutes();
                    let key = format!("ended:{}:{}", m.uid, m.start.timestamp());
                    if (ENDED_AFTER_MINUTES..ENDED_AFTER_MINUTES + 10).contains(&since_end)
                        && (m.end - m.start).num_minutes() >= 10
                        && announced.insert(key)
                    {
                        bus.publish(ended_event(m));
                    }
                }
                if announced.len() > 500 {
                    announced.clear();
                }
            }
        })
    }
}

fn lock(state: &Calendar) -> std::sync::MutexGuard<'_, CalendarState> {
    state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

async fn fetch(client: &reqwest::Client, url: &str) -> Result<String, String> {
    // Only real web links; Google shares webcal:// links for the same feed.
    let url = url.trim().replacen("webcal://", "https://", 1);
    if !url.starts_with("https://") {
        return Err("calendar links must start with https://".into());
    }
    let resp = client
        .get(&url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("calendar: {}", e.without_url()))?;
    let text = resp.text().await.map_err(|e| e.without_url().to_string())?;
    if !text.contains("BEGIN:VCALENDAR") {
        return Err("calendar link did not return an iCal feed".into());
    }
    Ok(text)
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

// ---------- iCal parsing ----------

struct Prop {
    name: String,
    params: Vec<(String, String)>,
    value: String,
}

impl Prop {
    fn param(&self, key: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
    }
}

/// Joins folded lines (a line starting with a space continues the last).
fn unfold(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix([' ', '\t'])
            && let Some(last) = out.last_mut()
        {
            last.push_str(rest);
        } else {
            out.push(line.trim_end_matches('\r').to_owned());
        }
    }
    out
}

fn parse_prop(line: &str) -> Option<Prop> {
    // The value starts at the first colon outside quotes.
    let mut quoted = false;
    let colon = line.char_indices().find_map(|(i, c)| {
        if c == '"' {
            quoted = !quoted;
        }
        (c == ':' && !quoted).then_some(i)
    })?;
    let (head, value) = (&line[..colon], &line[colon + 1..]);
    let mut parts = head.split(';');
    let name = parts.next()?.to_ascii_uppercase();
    let params = parts
        .filter_map(|p| p.split_once('='))
        .map(|(k, v)| (k.to_ascii_uppercase(), v.trim_matches('"').to_owned()))
        .collect();
    Some(Prop {
        name,
        params,
        value: value.to_owned(),
    })
}

fn unescape(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    let mut chars = v.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n' | 'N') => out.push('\n'),
                Some(other) => out.push(other),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// A DTSTART/DTEND value, or None for all-day dates.
fn parse_time(p: &Prop) -> Option<DateTime<Utc>> {
    if p.param("VALUE") == Some("DATE") || p.value.len() == 8 {
        return None;
    }
    let v = p.value.trim();
    if let Some(utc) = v.strip_suffix('Z') {
        let naive = NaiveDateTime::parse_from_str(utc, "%Y%m%dT%H%M%S").ok()?;
        return Some(Utc.from_utc_datetime(&naive));
    }
    let naive = NaiveDateTime::parse_from_str(v, "%Y%m%dT%H%M%S").ok()?;
    if let Some(tz) = p
        .param("TZID")
        .and_then(|t| t.parse::<chrono_tz::Tz>().ok())
    {
        return tz
            .from_local_datetime(&naive)
            .earliest()
            .map(|t| t.with_timezone(&Utc));
    }
    // Floating time or a Windows zone name: read it as this PC's time.
    Local
        .from_local_datetime(&naive)
        .earliest()
        .map(|t| t.with_timezone(&Utc))
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

#[derive(Default)]
struct Raw {
    uid: String,
    title: String,
    location: String,
    description: String,
    url: String,
    start: Option<Prop>,
    end: Option<DateTime<Utc>>,
    rrule: Vec<String>,
    exdates: Vec<String>,
    recurrence_id: Option<DateTime<Utc>>,
    cancelled: bool,
    attendees: Vec<String>,
}

/// Meetings starting between `from` and `to`, with recurring ones expanded.
pub fn parse(text: &str, from: DateTime<Utc>, to: DateTime<Utc>) -> Vec<Meeting> {
    let mut raws: Vec<Raw> = Vec::new();
    let mut current: Option<Raw> = None;
    for line in unfold(text) {
        if line.eq_ignore_ascii_case("BEGIN:VEVENT") {
            current = Some(Raw::default());
            continue;
        }
        if line.eq_ignore_ascii_case("END:VEVENT") {
            raws.extend(current.take());
            continue;
        }
        let (Some(raw), Some(p)) = (current.as_mut(), parse_prop(&line)) else {
            continue;
        };
        match p.name.as_str() {
            "UID" => raw.uid = p.value.clone(),
            "SUMMARY" => raw.title = unescape(&p.value),
            "LOCATION" => raw.location = unescape(&p.value),
            "DESCRIPTION" => raw.description = unescape(&p.value),
            "URL" => raw.url = p.value.clone(),
            "DTEND" => raw.end = parse_time(&p),
            "RRULE" | "RDATE" => raw.rrule.push(line.clone()),
            "EXDATE" => raw.exdates.push(line.clone()),
            "RECURRENCE-ID" => raw.recurrence_id = parse_time(&p),
            "STATUS" => raw.cancelled = p.value.eq_ignore_ascii_case("CANCELLED"),
            "ATTENDEE" => {
                if let Some(name) = p.param("CN") {
                    raw.attendees.push(name.to_owned());
                } else if let Some(mail) = p.value.strip_prefix("mailto:") {
                    raw.attendees.push(mail.to_owned());
                }
            }
            "DTSTART" => raw.start = Some(p),
            _ => {}
        }
    }

    // Moved or cancelled single occurrences replace the series' instance.
    let overrides: HashSet<(String, i64)> = raws
        .iter()
        .filter_map(|r| r.recurrence_id.map(|t| (r.uid.clone(), t.timestamp())))
        .collect();

    let mut out = Vec::new();
    for raw in raws {
        let Some(start_prop) = &raw.start else {
            continue;
        };
        let Some(start) = parse_time(start_prop) else {
            continue; // all-day
        };
        let length = raw.end.map_or(chrono::Duration::minutes(30), |e| e - start);
        let starts: Vec<DateTime<Utc>> = if raw.rrule.is_empty() || raw.recurrence_id.is_some() {
            vec![start]
        } else {
            expand(start_prop, start, &raw.rrule, &raw.exdates, from, to)
        };
        if raw.cancelled {
            continue;
        }
        let join_url = find_join_url(&[&raw.location, &raw.url, &raw.description]);
        for s in starts {
            if s < from || s > to {
                continue;
            }
            if raw.recurrence_id.is_none() && overrides.contains(&(raw.uid.clone(), s.timestamp()))
            {
                continue;
            }
            out.push(Meeting {
                uid: raw.uid.clone(),
                title: if raw.title.is_empty() {
                    "Meeting".into()
                } else {
                    raw.title.clone()
                },
                start: s,
                end: s + length,
                location: raw.location.clone(),
                description: raw.description.clone(),
                join_url: join_url.clone(),
                attendees: raw.attendees.clone(),
            });
        }
    }
    out.sort_by_key(|m| m.start);
    out
}

fn expand(
    start_prop: &Prop,
    start: DateTime<Utc>,
    rules: &[String],
    exdates: &[String],
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Vec<DateTime<Utc>> {
    // rrule wants an IANA zone; anything else is rewritten in UTC.
    let dtstart = match start_prop.param("TZID") {
        Some(tz) if tz.parse::<chrono_tz::Tz>().is_ok() => {
            format!("DTSTART;TZID={tz}:{}", start_prop.value.trim())
        }
        _ => format!("DTSTART:{}", start.format("%Y%m%dT%H%M%SZ")),
    };
    let mut spec = vec![dtstart];
    spec.extend(rules.iter().cloned());
    spec.extend(exdates.iter().cloned());
    let Ok(set) = spec.join("\n").parse::<rrule::RRuleSet>() else {
        return vec![start];
    };
    let tz = rrule::Tz::UTC;
    set.after(from.with_timezone(&tz))
        .before(to.with_timezone(&tz))
        .all(100)
        .dates
        .into_iter()
        .map(|d| d.with_timezone(&Utc))
        .collect()
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

    const ICS: &str = "BEGIN:VCALENDAR\r
VERSION:2.0\r
BEGIN:VEVENT\r
UID:one@x\r
SUMMARY:Design review\\, round 2\r
DTSTART:20261001T150000Z\r
DTEND:20261001T154500Z\r
LOCATION:https://meet.google.com/abc-defg-hij\r
ATTENDEE;CN=\"Sara Khan\";ROLE=REQ-PARTICIPANT:mailto:sara@x.com\r
ATTENDEE:mailto:ali@x.com\r
DESCRIPTION:Agenda:\\n- mocks\\n- next steps\r
END:VEVENT\r
BEGIN:VEVENT\r
UID:standup@x\r
SUMMARY:Standup\r
DTSTART;TZID=Asia/Karachi:20260901T100000\r
DTEND;TZID=Asia/Karachi:20260901T101500\r
RRULE:FREQ=DAILY;BYDAY=MO,TU,WE,TH,FR\r
DESCRIPTION:Join: https://us06web.zoom.us/j/123456?pwd=abc. Thanks\r
END:VEVENT\r
BEGIN:VEVENT\r
UID:standup@x\r
RECURRENCE-ID;TZID=Asia/Karachi:20261002T100000\r
SUMMARY:Standup (moved)\r
DTSTART;TZID=Asia/Karachi:20261002T113000\r
DTEND;TZID=Asia/Karachi:20261002T114500\r
END:VEVENT\r
BEGIN:VEVENT\r
UID:holiday@x\r
SUMMARY:Holiday\r
DTSTART;VALUE=DATE:20261001\r
END:VEVENT\r
BEGIN:VEVENT\r
UID:gone@x\r
SUMMARY:Cancelled thing\r
STATUS:CANCELLED\r
DTSTART:20261001T160000Z\r
END:VEVENT\r
END:VCALENDAR\r
";

    fn at(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn parses_single_recurring_and_moved_meetings() {
        let m = parse(ICS, at("2026-10-01T00:00:00Z"), at("2026-10-02T23:59:00Z"));
        let titles: Vec<(&str, String)> = m
            .iter()
            .map(|m| (m.title.as_str(), m.start.format("%d %H:%M").to_string()))
            .collect();
        assert_eq!(
            titles,
            vec![
                ("Standup", "01 05:00".to_owned()),
                ("Design review, round 2", "01 15:00".to_owned()),
                ("Standup (moved)", "02 06:30".to_owned()),
            ]
        );
        let review = &m[1];
        assert_eq!(
            review.join_url.as_deref(),
            Some("https://meet.google.com/abc-defg-hij")
        );
        assert_eq!(review.attendees, vec!["Sara Khan", "ali@x.com"]);
        assert_eq!(review.description, "Agenda:\n- mocks\n- next steps");
        assert_eq!((review.end - review.start).num_minutes(), 45);
        assert_eq!(
            m[0].join_url.as_deref(),
            Some("https://us06web.zoom.us/j/123456?pwd=abc")
        );
    }

    #[test]
    fn unfolds_long_lines() {
        let text =
            "BEGIN:VEVENT\nUID:a\nSUMMARY:Long\n  title\nDTSTART:20261001T150000Z\nEND:VEVENT\n";
        let m = parse(text, at("2026-10-01T00:00:00Z"), at("2026-10-02T00:00:00Z"));
        assert_eq!(m[0].title, "Long title");
        assert_eq!(
            (m[0].end - m[0].start).num_minutes(),
            30,
            "no DTEND means 30 min"
        );
    }

    #[test]
    fn finds_call_links_only_on_known_hosts() {
        assert_eq!(
            find_join_url(&[
                "",
                "Teams: <https://teams.microsoft.com/l/meetup-join/19%3a>"
            ]),
            Some("https://teams.microsoft.com/l/meetup-join/19%3a".into())
        );
        assert_eq!(
            find_join_url(&["https://evil.example/meet.google.com/x"]),
            None
        );
    }
}
