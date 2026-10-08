//! The name index for every drive: built in the background once a day,
//! on mains power and while Sidekick is not paused, then used by file
//! search in Ask and instant results. Searches read through their own
//! connection, so a build never makes them wait.
//!
//! When the optional indexer service is installed (Settings, "Find any
//! file on any drive") it keeps a live index in ProgramData that follows
//! every change; search reads that one while it is fresh and the daily
//! build is skipped.

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
static LIVE: Mutex<Option<NameIndex>> = Mutex::new(None);
/// The service marks the live index every minute; older means it stopped.
const LIVE_FRESH_MINUTES: i64 = 5;

/// Where the indexer service keeps its index.
pub fn live_db() -> PathBuf {
    let base = std::env::var_os("ProgramData").unwrap_or_else(|| "C:\\ProgramData".into());
    PathBuf::from(base).join("Sidekick").join("names-live.db")
}

/// True while the indexer service is running and keeping its index fresh.
pub fn live() -> bool {
    let mut live = crate::state::lock(&LIVE);
    if live.is_none() && cfg!(windows) && live_db().is_file() {
        *live = NameIndex::open_read_only(&live_db()).ok();
    }
    live.as_ref()
        .and_then(NameIndex::live)
        .is_some_and(|t| (chrono::Utc::now() - t).num_minutes() < LIVE_FRESH_MINUTES)
}

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
            if !live() && due() && sidekick_sensors::on_mains() && !crate::state::is_paused(&app) {
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
    if live()
        && let Some(idx) = crate::state::lock(&LIVE).as_ref()
    {
        return Some(idx.search(query, limit));
    }
    let reader = crate::state::lock(&READER);
    let idx = reader.as_ref()?;
    idx.built()?;
    Some(idx.search(query, limit))
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DriveIndexStatus {
    /// The indexer ships with this build (Windows release builds).
    available: bool,
    live: bool,
}

#[tauri::command]
pub fn drive_index_status() -> DriveIndexStatus {
    DriveIndexStatus {
        available: cfg!(windows) && indexer_exe().is_some_and(|p| p.is_file()),
        live: live(),
    }
}

fn indexer_exe() -> Option<PathBuf> {
    Some(
        std::env::current_exe()
            .ok()?
            .with_file_name("sidekick-indexer.exe"),
    )
}

/// Installs or removes the indexer service. Windows asks for admin
/// approval once; nothing else in Sidekick runs elevated.
#[tauri::command]
pub async fn drive_index_set(on: bool) -> Result<(), String> {
    let exe = indexer_exe()
        .filter(|p| p.is_file())
        .ok_or("The drive indexer is not installed with this build.")?;
    let args = if on {
        let home = dirs::home_dir().unwrap_or_default();
        format!(
            "'install','--profile','{}'",
            home.display().to_string().replace('\'', "''")
        )
    } else {
        "'uninstall'".to_string()
    };
    let script = format!(
        "$p = Start-Process -FilePath '{}' -ArgumentList {args} -Verb RunAs -WindowStyle Hidden -Wait -PassThru; exit $p.ExitCode",
        exe.display().to_string().replace('\'', "''")
    );
    tauri::async_runtime::spawn_blocking(move || {
        let mut cmd = std::process::Command::new("powershell");
        cmd.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let out = cmd.output().map_err(|e| e.to_string())?;
        if out.status.success() {
            *crate::state::lock(&LIVE) = None;
            Ok(())
        } else {
            Err("Windows did not allow it, or the indexer could not start.".to_string())
        }
    })
    .await
    .map_err(|e| e.to_string())?
}
