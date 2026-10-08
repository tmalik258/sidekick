//! Focus mode: a timed stretch with Do Not Disturb on, notifications held
//! and suggestions waiting. Started by voice ("focus for 45 minutes"), a
//! shortcut, the tray or the island. When it ends, the island lists what
//! was held.

use std::sync::Mutex;

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// The island shows the focus pill while this carries `until`, and the
/// held list when it carries `ended`.
pub const EVENT: &str = "island://focus";
/// Minutes when none are given.
pub const DEFAULT_MINUTES: u32 = 25;

struct Focus {
    until: DateTime<Utc>,
    held: Vec<Held>,
    /// Focus turned Do Not Disturb on, so it turns it off again.
    set_dnd: bool,
    /// Changes when a new focus starts, so an old timer does not end it.
    run: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Held {
    pub title: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// Milliseconds since the epoch; None when not focusing.
    pub until: Option<i64>,
    pub held: usize,
}

static FOCUS: Mutex<Option<Focus>> = Mutex::new(None);
static RUNS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn focus() -> std::sync::MutexGuard<'static, Option<Focus>> {
    FOCUS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Whether a focus stretch is running.
pub fn active() -> bool {
    focus().as_ref().is_some_and(|f| f.until > Utc::now())
}

pub fn status() -> Status {
    let f = focus();
    match f.as_ref().filter(|f| f.until > Utc::now()) {
        Some(f) => Status {
            until: Some(f.until.timestamp_millis()),
            held: f.held.len(),
        },
        None => Status {
            until: None,
            held: 0,
        },
    }
}

/// Keeps something for the end-of-focus list. False when not focusing.
pub fn hold(title: &str, detail: &str) -> bool {
    let mut f = focus();
    match f.as_mut().filter(|f| f.until > Utc::now()) {
        Some(f) => {
            f.held.push(Held {
                title: title.to_owned(),
                detail: detail.to_owned(),
            });
            true
        }
        None => false,
    }
}

/// Starts (or restarts) focus for `minutes`. Returns what to show or say.
pub fn start(app: &AppHandle, minutes: u32) -> String {
    let minutes = minutes.clamp(1, 8 * 60);
    let run = RUNS.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
    let until = Utc::now() + Duration::minutes(i64::from(minutes));
    let already = {
        let mut f = focus();
        let held = f.take().map(|old| (old.held, old.set_dnd));
        let (held, set_dnd) = held.unwrap_or_default();
        *f = Some(Focus {
            until,
            held,
            set_dnd,
            run,
        });
        set_dnd
    };
    if !already {
        let app = app.clone();
        std::thread::spawn(move || {
            let was_on = sidekick_actions::dnd::state().unwrap_or(false);
            if !was_on
                && sidekick_actions::pc::set_dnd(true).is_ok()
                && let Some(f) = focus().as_mut().filter(|f| f.run == run)
            {
                f.set_dnd = true;
            }
            emit(&app);
        });
    }
    // Ends on its own when the time is up.
    let app2 = app.clone();
    std::thread::spawn(move || {
        let wait = (until - Utc::now()).to_std().unwrap_or_default();
        std::thread::sleep(wait);
        let same = focus().as_ref().is_some_and(|f| f.run == run);
        if same {
            stop(&app2);
        }
    });
    emit(app);
    format!("Focusing for {}", plain_minutes(minutes))
}

/// Ends focus: Do Not Disturb back off if focus turned it on, and the
/// island lists what was held. Returns what to show or say.
pub fn stop(app: &AppHandle) -> String {
    let Some(f) = focus().take() else {
        return "Not focusing".into();
    };
    if f.set_dnd {
        std::thread::spawn(|| {
            let _ = sidekick_actions::pc::set_dnd(false);
        });
    }
    let _ = app.emit(
        EVENT,
        serde_json::json!({ "until": null, "ended": true, "held": f.held }),
    );
    if !f.held.is_empty() {
        offer_held(app, &f.held);
    }
    match f.held.len() {
        0 => "Focus done. Nothing came in.".into(),
        1 => "Focus done. 1 thing waited for you.".into(),
        n => format!("Focus done. {n} things waited for you."),
    }
}

/// A card naming what waited, newest last. They also stay in "Saved for
/// later" on the hover card.
fn offer_held(app: &AppHandle, held: &[Held]) {
    use sidekick_skills::{Proposal, ProposedOption, Trust};
    let mut lines: Vec<String> = held.iter().take(5).map(|h| h.title.clone()).collect();
    if held.len() > 5 {
        lines.push(format!("and {} more in Saved for later", held.len() - 5));
    }
    crate::suggestions::offer(
        app,
        Proposal {
            skill_id: "focus.done".into(),
            skill_ids: vec!["focus.done".into()],
            title: "While you focused".into(),
            detail: lines.join("\n"),
            options: vec![ProposedOption {
                label: "Got it".into(),
                action: "noop".into(),
                args: serde_json::json!({ "message": "OK" }),
                skill_id: "focus.done".into(),
            }],
            trust: Trust::Suggest,
            remember: None,
            priority: 60,
        },
    );
}

fn emit(app: &AppHandle) {
    let s = status();
    let _ = app.emit(
        EVENT,
        serde_json::json!({ "until": s.until, "held": s.held }),
    );
}

fn plain_minutes(m: u32) -> String {
    match (m / 60, m % 60) {
        (0, 1) => "1 minute".into(),
        (0, m) => format!("{m} minutes"),
        (1, 0) => "1 hour".into(),
        (h, 0) => format!("{h} hours"),
        (h, m) => format!("{h} h {m} min"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minutes_read_plainly() {
        assert_eq!(plain_minutes(25), "25 minutes");
        assert_eq!(plain_minutes(60), "1 hour");
        assert_eq!(plain_minutes(90), "1 h 30 min");
        assert_eq!(plain_minutes(120), "2 hours");
    }
}
