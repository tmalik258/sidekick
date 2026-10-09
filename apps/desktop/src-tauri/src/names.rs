//! The name index for every drive: built in the background once a day,
//! on mains power and while Sidekick is not paused, then used by file
//! search in Ask and instant results. Searches read through their own
//! connection, so a build never makes them wait.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use sidekick_core::names::{NameHit, NameIndex};
use tauri::{AppHandle, Manager};

use crate::state::AppState;

/// First build waits a little after start, so startup stays quick.
const FIRST_WAIT: Duration = Duration::from_secs(3 * 60);
const CHECK_EVERY: Duration = Duration::from_secs(30 * 60);
const REBUILD_AFTER_HOURS: i64 = 24;

static READER: Mutex<Option<NameIndex>> = Mutex::new(None);

fn db(app: &AppHandle) -> PathBuf {
    app.state::<AppState>().data_dir.join("names.db")
}

/// Every fixed drive on this PC ("C:\", "D:\"); the home folder elsewhere.
pub fn drive_roots() -> Vec<PathBuf> {
    if cfg!(windows) {
        ('C'..='Z')
            .map(|c| PathBuf::from(format!("{c}:\\")))
            .filter(|p| p.is_dir())
            .collect()
    } else {
        dirs::home_dir().into_iter().collect()
    }
}

pub fn start(app: &AppHandle) {
    match NameIndex::open(&db(app)) {
        Ok(idx) => *crate::state::lock(&READER) = Some(idx),
        Err(e) => {
            log::warn!("name index: {e}");
            return;
        }
    }
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(FIRST_WAIT);
        loop {
            if due() && sidekick_sensors::on_mains() && !crate::state::is_paused(&app) {
                build(&app);
            }
            std::thread::sleep(CHECK_EVERY);
        }
    });
}

fn due() -> bool {
    let built = crate::state::lock(&READER)
        .as_ref()
        .and_then(NameIndex::built);
    built.is_none_or(|t| (chrono::Utc::now() - t).num_hours() >= REBUILD_AFTER_HOURS)
}

fn build(app: &AppHandle) {
    let Ok(mut writer) = NameIndex::open(&db(app)) else {
        return;
    };
    let started = std::time::Instant::now();
    // Stops (keeping the old index) if the PC goes on battery or Sidekick is paused.
    let result = writer.rebuild(&drive_roots(), || {
        sidekick_sensors::on_mains() && !crate::state::is_paused(app)
    });
    match result {
        Ok(Some(n)) => log::info!("name index: {n} entries in {:?}", started.elapsed()),
        Ok(None) => log::info!("name index: stopped, will try again"),
        Err(e) => log::warn!("name index: {e}"),
    }
}

/// Matches from the index, or None while it has never been built.
pub fn search(query: &str, limit: usize) -> Option<Vec<NameHit>> {
    let reader = crate::state::lock(&READER);
    let idx = reader.as_ref()?;
    idx.built()?;
    Some(idx.search(query, limit))
}
