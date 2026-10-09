//! Freeze watch: notes anything that holds the UI thread long enough to
//! show. Commands that run there are timed one by one, and a heartbeat from
//! another thread catches stalls from anything else (events, tray, timers),
//! naming the command that was running when it can. Entries go to the log
//! and to `freeze_report`, which diagnostics and the end-to-end tests read.
//!
//! The heartbeat uses `run_on_main_thread`. On Windows that path has been
//! linked to intermittent heap corruption in Tauri/tao when hammered, so the
//! beat is slow (seconds, not fractions), and `SIDEKICK_NO_FREEZE_WATCH=1`
//! turns it off for A/B. Sync command timing always stays on.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::ipc::Invoke;
use tauri::{AppHandle, Runtime};

/// Longer than this on the UI thread is a dropped frame or worse.
pub const SLOW: Duration = Duration::from_millis(50);
/// Heartbeat interval: rare enough not to stress the event loop.
#[cfg(debug_assertions)]
const BEAT: Duration = Duration::from_secs(2);
#[cfg(not(debug_assertions))]
const BEAT: Duration = Duration::from_secs(5);
/// Kept for the report.
const KEEP: usize = 50;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Freeze {
    /// The command that held the thread, or "stall" from the heartbeat.
    pub what: String,
    pub ms: u64,
    /// Seconds since Sidekick started.
    pub at: u64,
}

/// Counts for diagnostics: named invokes vs unlabeled stalls.
#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct FreezeSummary {
    pub commands: usize,
    pub stalls: usize,
    /// Longest hold still in the ring, milliseconds.
    pub worst_ms: u64,
}

static SEEN: Mutex<VecDeque<Freeze>> = Mutex::new(VecDeque::new());
/// The command running on the UI thread right now, for naming a stall.
static RUNNING: Mutex<Option<String>> = Mutex::new(None);
static STARTED: Mutex<Option<Instant>> = Mutex::new(None);

fn note(what: String, took: Duration) {
    let started = *STARTED.lock().unwrap_or_else(|e| e.into_inner());
    let at = started.map_or(0, |s| s.elapsed().as_secs());
    let ms = took.as_millis() as u64;
    log::warn!("UI thread held {ms} ms by {what}");
    let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    if seen.len() == KEEP {
        seen.pop_front();
    }
    seen.push_back(Freeze { what, ms, at });
}

/// Everything slow seen so far, oldest first.
pub fn report() -> Vec<Freeze> {
    SEEN.lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .cloned()
        .collect()
}

/// How many named command holds vs unlabeled stalls are in the ring.
pub fn summary() -> FreezeSummary {
    let seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    let mut out = FreezeSummary::default();
    for f in seen.iter() {
        if f.what == "stall" {
            out.stalls += 1;
        } else {
            out.commands += 1;
        }
        out.worst_ms = out.worst_ms.max(f.ms);
    }
    out
}

/// Wraps the command handler: commands that run on the UI thread (the ones
/// that are not async) are timed, and the name is kept while they run.
pub fn timed<R: Runtime>(
    handler: impl Fn(Invoke<R>) -> bool + Send + Sync + 'static,
) -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
    move |invoke| {
        let name = invoke.message.command().to_owned();
        *RUNNING.lock().unwrap_or_else(|e| e.into_inner()) = Some(name.clone());
        let started = Instant::now();
        let handled = handler(invoke);
        let took = started.elapsed();
        *RUNNING.lock().unwrap_or_else(|e| e.into_inner()) = None;
        if took >= SLOW {
            note(name, took);
        }
        handled
    }
}

fn watch_disabled() -> bool {
    match std::env::var("SIDEKICK_NO_FREEZE_WATCH") {
        Ok(v) => {
            let v = v.trim();
            !v.is_empty() && v != "0" && !v.eq_ignore_ascii_case("false")
        }
        Err(_) => false,
    }
}

/// Starts the heartbeat: periodically asks the UI thread to answer, and notes
/// how long it took when that is over [`SLOW`]. No-op when
/// `SIDEKICK_NO_FREEZE_WATCH` is set (command timing still runs).
pub fn watch<R: Runtime>(app: &AppHandle<R>) {
    *STARTED.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
    if watch_disabled() {
        log::info!("freeze heartbeat off (SIDEKICK_NO_FREEZE_WATCH)");
        return;
    }
    log::info!(
        "freeze heartbeat every {} ms",
        BEAT.as_millis()
    );
    let app = app.clone();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(BEAT);
            let (tx, rx) = mpsc::channel();
            let sent = Instant::now();
            if app
                .run_on_main_thread(move || {
                    let _ = tx.send(());
                })
                .is_err()
            {
                return;
            }
            // The command running when it went quiet is the likely cause.
            let mut cause = None;
            loop {
                match rx.recv_timeout(SLOW) {
                    Ok(()) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        if cause.is_none() {
                            cause = RUNNING.lock().unwrap_or_else(|e| e.into_inner()).clone();
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
            }
            let took = sent.elapsed();
            // A timed command already noted itself when it returned.
            if took >= SLOW * 2 && cause.is_none() {
                note("stall".to_owned(), took);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    static TEST: Mutex<()> = Mutex::new(());

    fn clear_seen() {
        SEEN.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }

    #[test]
    fn keeps_the_last_entries() {
        let _g = TEST.lock().unwrap_or_else(|e| e.into_inner());
        clear_seen();
        for i in 0..(KEEP + 5) {
            note(format!("cmd{i}"), Duration::from_millis(60));
        }
        let r = report();
        assert_eq!(r.len(), KEEP);
        assert_eq!(r.last().unwrap().what, format!("cmd{}", KEEP + 4));
        assert!(r.iter().all(|f| f.ms == 60));
    }

    #[test]
    fn summary_splits_commands_and_stalls() {
        let _g = TEST.lock().unwrap_or_else(|e| e.into_inner());
        clear_seen();
        note("settings_set".into(), Duration::from_millis(80));
        note("stall".into(), Duration::from_millis(120));
        note("stall".into(), Duration::from_millis(200));
        let s = summary();
        assert_eq!(s.commands, 1);
        assert_eq!(s.stalls, 2);
        assert_eq!(s.worst_ms, 200);
    }

    #[test]
    fn env_gate_treats_zero_as_on() {
        let off = |v: &str| {
            let v = v.trim();
            !v.is_empty() && v != "0" && !v.eq_ignore_ascii_case("false")
        };
        assert!(off("1"));
        assert!(off("true"));
        assert!(!off("0"));
        assert!(!off("false"));
        assert!(!off(""));
    }
}
