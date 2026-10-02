//! Noticing when you are stuck: the same error copied again within a few
//! minutes raises `dev.stuck`, which offers help in Ask without being asked.

use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use sidekick_core::Event;
use tauri::{AppHandle, Manager};

use crate::state::{AppState, lock};

pub const STUCK: &str = "dev.stuck";
/// The same error again within this long counts as being stuck.
const WINDOW: Duration = Duration::from_secs(10 * 60);
/// Two copies this close are one action (copy, paste, copy), not a retry.
const SAME_MOMENT: Duration = Duration::from_secs(45);
const KEEP: usize = 20;

static SEEN: LazyLock<Mutex<Vec<(String, Instant)>>> = LazyLock::new(Mutex::default);

/// The part of an error that stays the same between runs: its first
/// line, without numbers (line numbers, ports, ids change).
pub fn fingerprint(text: &str) -> Option<String> {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty())?;
    let fp: String = line
        .chars()
        .filter(|c| !c.is_ascii_digit())
        .take(120)
        .collect::<String>()
        .to_lowercase();
    (fp.len() >= 8).then_some(fp)
}

/// Minutes since the same error was first seen, if this is a retry.
fn repeated(seen: &mut Vec<(String, Instant)>, fp: &str, now: Instant) -> Option<u64> {
    seen.retain(|(_, at)| now.duration_since(*at) < WINDOW);
    let first = seen.iter().find(|(f, _)| f == fp).map(|(_, at)| *at);
    match first {
        Some(at) if now.duration_since(at) >= SAME_MOMENT => {
            // Offer once per error; a new run starts the count again.
            seen.retain(|(f, _)| f != fp);
            Some(now.duration_since(at).as_secs().div_ceil(60))
        }
        Some(_) => None,
        None => {
            seen.push((fp.to_owned(), now));
            if seen.len() > KEEP {
                seen.remove(0);
            }
            None
        }
    }
}

pub fn observe(app: &AppHandle, event: &Event) {
    if event.kind != "clipboard.changed" || event.payload["kind"].as_str() != Some("stack_trace") {
        return;
    }
    let text = event.payload["text"].as_str().unwrap_or_default();
    let Some(fp) = fingerprint(text) else { return };
    let minutes = repeated(&mut lock(&SEEN), &fp, Instant::now());
    if let Some(minutes) = minutes {
        app.state::<AppState>().bus.publish(Event::new(
            STUCK,
            "clipboard",
            serde_json::json!({
                "preview": event.payload["preview"],
                "minutes": minutes,
            }),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_error_again_is_stuck() {
        let a =
            fingerprint("TypeError: x is undefined at line 42\n  at foo (app.js:42:7)").unwrap();
        let b =
            fingerprint("TypeError: x is undefined at line 57\n  at foo (app.js:57:3)").unwrap();
        assert_eq!(a, b, "line numbers do not count");
        let mut seen = Vec::new();
        let t0 = Instant::now();
        assert_eq!(repeated(&mut seen, &a, t0), None, "first time");
        assert_eq!(
            repeated(&mut seen, &a, t0 + Duration::from_secs(10)),
            None,
            "same moment"
        );
        assert_eq!(
            repeated(&mut seen, &a, t0 + Duration::from_secs(4 * 60)),
            Some(4)
        );
        assert_eq!(
            repeated(&mut seen, &a, t0 + Duration::from_secs(5 * 60)),
            None,
            "offered once"
        );
        assert!(fingerprint("  \n").is_none());
    }
}
