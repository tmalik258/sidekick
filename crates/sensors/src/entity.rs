//! Everyday things in copied text: a street address, a phone number, a
//! date. Each comes with what a card needs (a maps link, the number in a
//! clean form, a calendar link), so skills only pick buttons.

use std::sync::LazyLock;

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime};
use regex::Regex;
use serde_json::{Value, json};

/// Longer text is a document, not an address or a date.
const MAX_LEN: usize = 160;

static ADDRESS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^(?:(?:house|flat|apt|suite|unit|plot)\s*#?\s*)?\d{1,6}[a-z]?,?\s+[\w.'\- ]{2,60}\b(street|st|avenue|ave|road|rd|boulevard|blvd|lane|ln|drive|dr|way|court|ct|place|pl|block|sector|phase|highway|hwy|square|sq)\b",
    )
    .expect("address regex")
});
static IPV4: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})(?::(\d{1,5}))?(?:/(\d{1,2}))?$")
        .expect("ipv4 regex")
});
static PHONE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\+?\(?\d[\d\s().\-]{6,20}\d$").expect("phone regex"));
static ISO_DATE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(\d{4})-(\d{2})-(\d{2})(?:[ T](\d{1,2}):(\d{2}))?$").expect("iso date regex")
});
static MONTH_DATE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?i)^(?:(?:mon|tue|wed|thu|fri|sat|sun)[a-z]*,?\s+)?",
        r"(?:(?P<d1>\d{1,2})(?:st|nd|rd|th)?\s+(?P<m1>[a-z]{3,9})|(?P<m2>[a-z]{3,9})\s+(?P<d2>\d{1,2})(?:st|nd|rd|th)?)",
        r",?(?:\s+(?P<y>\d{4}))?",
        r"(?:,?\s+(?:at\s+)?(?P<h>\d{1,2})(?::(?P<min>\d{2}))?\s*(?P<ap>am|pm)?)?\s*$",
    ))
    .expect("month date regex")
});

const MONTHS: &[&str] = &[
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];

/// Percent-encodes a query value.
pub fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn month(name: &str) -> Option<u32> {
    let lower = name.to_ascii_lowercase();
    MONTHS
        .iter()
        .position(|m| lower.starts_with(m))
        .map(|i| i as u32 + 1)
}

/// A date (and maybe a time) in `text`. A date without a year is the next
/// one from `today`.
pub fn parse_date(text: &str, today: NaiveDate) -> Option<(NaiveDate, Option<NaiveTime>)> {
    let t = text.trim();
    if let Some(c) = ISO_DATE.captures(t) {
        let date =
            NaiveDate::from_ymd_opt(c[1].parse().ok()?, c[2].parse().ok()?, c[3].parse().ok()?)?;
        let time = match (c.get(4), c.get(5)) {
            (Some(h), Some(m)) => {
                NaiveTime::from_hms_opt(h.as_str().parse().ok()?, m.as_str().parse().ok()?, 0)
            }
            _ => None,
        };
        return Some((date, time));
    }
    let c = MONTH_DATE.captures(t)?;
    let (d, m) = match (c.name("d1"), c.name("m1"), c.name("m2"), c.name("d2")) {
        (Some(d), Some(m), _, _) | (_, _, Some(m), Some(d)) => (d.as_str(), m.as_str()),
        _ => return None,
    };
    let m = month(m)?;
    let d: u32 = d.parse().ok()?;
    let date = match c.name("y") {
        Some(y) => NaiveDate::from_ymd_opt(y.as_str().parse().ok()?, m, d)?,
        None => {
            let this = NaiveDate::from_ymd_opt(today.year(), m, d)?;
            if this < today {
                NaiveDate::from_ymd_opt(today.year() + 1, m, d)?
            } else {
                this
            }
        }
    };
    let time = match c.name("h") {
        Some(h) => {
            let mut hour: u32 = h.as_str().parse().ok()?;
            let minute: u32 = c.name("min").map_or(Some(0), |m| m.as_str().parse().ok())?;
            match c.name("ap").map(|a| a.as_str().to_ascii_lowercase()) {
                Some(ap) if ap == "pm" && hour < 12 => hour += 12,
                Some(ap) if ap == "am" && hour == 12 => hour = 0,
                None if c.name("min").is_none() => return Some((date, None)),
                _ => {}
            }
            NaiveTime::from_hms_opt(hour, minute, 0)
        }
        None => None,
    };
    Some((date, time))
}

/// Google Calendar's "new event" link for a date (an hour long with a
/// time, all day without).
pub fn calendar_url(title: &str, date: NaiveDate, time: Option<NaiveTime>) -> String {
    let dates = match time {
        Some(t) => {
            let start = NaiveDateTime::new(date, t);
            let end = start + Duration::hours(1);
            format!(
                "{}/{}",
                start.format("%Y%m%dT%H%M%S"),
                end.format("%Y%m%dT%H%M%S")
            )
        }
        None => format!(
            "{}/{}",
            date.format("%Y%m%d"),
            (date + Duration::days(1)).format("%Y%m%d")
        ),
    };
    format!(
        "https://calendar.google.com/calendar/render?action=TEMPLATE&text={}&dates={dates}",
        encode(title)
    )
}

/// An IPv4 address (with an optional :port or /prefix), and its parts.
fn ip(t: &str) -> Option<Value> {
    let c = IPV4.captures(t)?;
    let octets: Vec<u8> = (1..=4)
        .map(|i| c[i].parse::<u8>().ok())
        .collect::<Option<_>>()?;
    let ip = octets
        .iter()
        .map(u8::to_string)
        .collect::<Vec<_>>()
        .join(".");
    let port = c.get(5).map(|p| p.as_str().to_owned());
    if port.as_deref().and_then(|p| p.parse::<u16>().ok()) == Some(0) {
        return None;
    }
    let private = matches!(
        octets[..],
        [10, ..] | [127, ..] | [192, 168, ..] | [169, 254, ..]
    ) || (octets[0] == 172 && (16..=31).contains(&octets[1]))
        || (octets[0] == 100 && (64..=127).contains(&octets[1]));
    let host = match &port {
        Some(p) => format!("{ip}:{p}"),
        None => ip.clone(),
    };
    Some(json!({
        "entity": "ip",
        "ip": ip,
        "port": port,
        "private": if private { "yes" } else { "no" },
        "lookup_url": format!("https://ipinfo.io/{ip}"),
        "web_url": format!("http://{host}"),
        "ssh": format!("ssh root@{ip}"),
    }))
}

/// What kind of everyday thing `text` is, with the fields its card needs.
pub fn detect(text: &str, today: NaiveDate) -> Option<Value> {
    let t = text.trim();
    if t.is_empty() || t.chars().count() > MAX_LEN || t.lines().count() > 4 {
        return None;
    }
    let one_line = t
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(", ");
    // Before phones: "37.27.195.216" has enough digits to look like one.
    if !t.contains('\n')
        && let Some(v) = ip(t)
    {
        return Some(v);
    }
    if let Some((date, time)) = parse_date(&one_line, today) {
        let when = match time {
            Some(tm) => format!("{} {}", date.format("%a %-d %b %Y"), tm.format("%H:%M")),
            None => date.format("%a %-d %b %Y").to_string(),
        };
        return Some(json!({
            "entity": "date",
            "when": when,
            "calendar_url": calendar_url("New event", date, time),
        }));
    }
    if !t.contains('\n') && PHONE.is_match(t) {
        let digits: String = t.chars().filter(char::is_ascii_digit).collect();
        let separated = t.chars().any(|c| matches!(c, ' ' | '-' | '(' | '.'));
        if (10..=15).contains(&digits.len()) && (t.starts_with('+') || separated) {
            let plus = if t.starts_with('+') { "+" } else { "" };
            return Some(json!({
                "entity": "phone",
                "number": format!("{plus}{digits}"),
                "whatsapp_url": format!("https://wa.me/{digits}"),
            }));
        }
    }
    if ADDRESS.is_match(&one_line) {
        return Some(json!({
            "entity": "address",
            "address": one_line,
            "maps_url": format!("https://www.google.com/maps/search/?api=1&query={}", encode(&one_line)),
        }));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 2).unwrap()
    }

    #[test]
    fn finds_dates() {
        let (d, t) = parse_date("Oct 5 at 3pm", today()).unwrap();
        assert_eq!(d, NaiveDate::from_ymd_opt(2026, 10, 5).unwrap());
        assert_eq!(t, NaiveTime::from_hms_opt(15, 0, 0));
        let (d, t) = parse_date("Monday, 12th January", today()).unwrap();
        assert_eq!(d, NaiveDate::from_ymd_opt(2027, 1, 12).unwrap());
        assert_eq!(t, None);
        let (_, t) = parse_date("2026-11-03 09:30", today()).unwrap();
        assert_eq!(t, NaiveTime::from_hms_opt(9, 30, 0));
        assert!(parse_date("the meeting went well", today()).is_none());
        assert!(parse_date("May I ask", today()).is_none());
        let url = calendar_url("New event", d, t);
        assert!(url.ends_with("dates=20270112T093000/20270112T103000"));
    }

    #[test]
    fn finds_ip_addresses() {
        let v = detect("37.27.195.216", today()).unwrap();
        assert_eq!(v["entity"], "ip");
        assert_eq!(v["private"], "no");
        assert_eq!(v["lookup_url"], "https://ipinfo.io/37.27.195.216");
        assert_eq!(v["ssh"], "ssh root@37.27.195.216");
        let local = detect("192.168.1.10:8080", today()).unwrap();
        assert_eq!(local["private"], "yes");
        assert_eq!(local["web_url"], "http://192.168.1.10:8080");
        assert_eq!(detect("10.0.0.0/24", today()).unwrap()["ip"], "10.0.0.0");
        assert_ne!(
            detect("999.1.1.1", today()).map(|v| v["entity"].clone()),
            Some(json!("ip")),
            "not an address"
        );
    }

    #[test]
    fn finds_phones_and_addresses() {
        let p = detect("+92 300 123-4567", today()).unwrap();
        assert_eq!(p["entity"], "phone");
        assert_eq!(p["number"], "+923001234567");
        assert!(
            detect("1234567890123", today()).is_none(),
            "a bare id is not a phone"
        );
        let a = detect("221B Baker Street\nLondon NW1 6XE", today()).unwrap();
        assert_eq!(a["entity"], "address");
        assert_eq!(a["address"], "221B Baker Street, London NW1 6XE");
        assert!(
            a["maps_url"]
                .as_str()
                .unwrap()
                .contains("221B%20Baker%20Street")
        );
        assert!(detect("I walked down the street", today()).is_none());
    }
}
