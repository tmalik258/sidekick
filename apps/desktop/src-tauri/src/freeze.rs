//! Freeze watch: notes anything that holds the UI thread long enough to
//! show. Commands that run there are timed one by one, and a heartbeat from
//! another thread catches stalls from anything else (events, tray, timers),
//! naming the command that was running when it can. Entries go to the log
//! and to `freeze_report`, which diagnostics and the end-to-end tests read.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::ipc::Invoke;
use tauri::{AppHandle, Runtime};

/// Longer than this on the UI thread is a dropped frame or worse.
pub const SLOW: Duration = Duration::from_millis(50);
/// How often the heartbeat checks the UI thread.
const BEAT: Duration = Duration::from_millis(250);
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

/// Starts the heartbeat: every quarter second it asks the UI thread to
/// answer, and notes how long it took when that is over [`SLOW`].
pub fn watch<R: Runtime>(app: &AppHandle<R>) {
    *STARTED.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
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
            // A timed command already noted itself.
            if took >= SLOW * 2 && cause.is_none() {
                note("stall".to_owned(), took);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_last_entries() {
        for i in 0..(KEEP + 5) {
            note(format!("cmd{i}"), Duration::from_millis(60));
        }
        let r = report();
        assert_eq!(r.len(), KEEP);
        assert_eq!(r.last().unwrap().what, format!("cmd{}", KEEP + 4));
        assert!(r.iter().all(|f| f.ms == 60));
    }
}
