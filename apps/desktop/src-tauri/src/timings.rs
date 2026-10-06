//! How fast Sidekick feels, measured where it matters: Ask open to ready,
//! Enter to the first word, end of speech to the first sound. Kept in a
//! small log on this PC (never sent anywhere) and shown in the timings
//! overlay (Ctrl+Alt+Shift+T in Ask).

use std::io::Write;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::state::AppState;

pub const EVENT: &str = "timing://recorded";
pub const OPEN_TO_READY: &str = "open_to_ready";
pub const ENTER_TO_FIRST_WORD: &str = "enter_to_first_word";
pub const SPEECH_TO_FIRST_SOUND: &str = "speech_to_first_sound";
const NAMES: &[&str] = &[OPEN_TO_READY, ENTER_TO_FIRST_WORD, SPEECH_TO_FIRST_SOUND];

const FILE: &str = "timings.log";
/// The log is cut to its newer half past this size.
const MAX_BYTES: u64 = 256 * 1024;
/// Longer than this is a stall or a bug in the measuring, not a timing.
const MAX_MS: u64 = 120_000;

#[derive(Debug, Clone, Serialize)]
pub struct Timing {
    pub name: &'static str,
    pub ms: u64,
}

/// Records one timing. Unknown names and silly values are ignored.
pub fn record(app: &AppHandle, name: &str, ms: u64) {
    let Some(name) = NAMES.iter().copied().find(|n| *n == name) else {
        return;
    };
    if ms > MAX_MS {
        return;
    }
    let _ = app.emit(EVENT, Timing { name, ms });
    let path = app.state::<AppState>().data_dir.join(FILE);
    trim(&path);
    let line = serde_json::json!({
        "ts": chrono::Utc::now().to_rfc3339(),
        "name": name,
        "ms": ms,
    });
    let written = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| writeln!(f, "{line}"));
    if let Err(err) = written {
        log::debug!("could not log a timing: {err}");
    }
}

fn trim(path: &std::path::Path) {
    let big = std::fs::metadata(path).is_ok_and(|m| m.len() > MAX_BYTES);
    if !big {
        return;
    }
    if let Ok(text) = std::fs::read_to_string(path) {
        let lines: Vec<&str> = text.lines().collect();
        let keep = lines[lines.len() / 2..].join("\n");
        let _ = std::fs::write(path, keep + "\n");
    }
}

/// The last `n` timings from the log, oldest first.
pub fn recent(app: &AppHandle, n: usize) -> Vec<Timing> {
    let path = app.state::<AppState>().data_dir.join(FILE);
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut out: Vec<Timing> = text
        .lines()
        .rev()
        .filter_map(|l| {
            let v: serde_json::Value = serde_json::from_str(l).ok()?;
            let name = NAMES.iter().copied().find(|n| v["name"] == *n)?;
            Some(Timing {
                name,
                ms: v["ms"].as_u64()?,
            })
        })
        .take(n)
        .collect();
    out.reverse();
    out
}
