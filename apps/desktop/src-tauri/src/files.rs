//! File helpers that need the app: document summaries in Ask mode
//! (FR-FILE-06) and the weekly Downloads check (FR-FILE-07).

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

use chrono::{Local, Timelike};
use sidekick_core::Event;
use tauri::{AppHandle, Manager};

use crate::state::{AppState, executor, lock};

pub const DOWNLOADS_OLD: &str = "files.downloads_old";
const MAX_TEXT: usize = 30_000;

fn run(exe: &Path, args: &[&std::ffi::OsStr]) -> Result<String, String> {
    let mut cmd = Command::new(exe);
    cmd.args(args).stdin(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let out = cmd.output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err("could not read the document".into());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The text of a PDF, Word or text file.
fn text_of(app: &AppHandle, path: &Path) -> Result<String, String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let exec = executor(&app.state::<AppState>());
    let caps = exec.capabilities();
    match ext.as_str() {
        "txt" | "md" => std::fs::read_to_string(path).map_err(|e| e.to_string()),
        "pdf" => {
            let tool = caps
                .pdftotext
                .clone()
                .ok_or("Install Poppler (pdftotext) to summarize PDFs: winget install oschwartz10612.Poppler")?;
            run(
                &tool,
                &[
                    "-layout".as_ref(),
                    "-q".as_ref(),
                    path.as_os_str(),
                    "-".as_ref(),
                ],
            )
        }
        "docx" | "odt" | "rtf" => {
            let tool = caps.pandoc.clone().ok_or(
                "Install pandoc to summarize Word files: winget install JohnMacFarlane.Pandoc",
            )?;
            run(&tool, &["-t".as_ref(), "plain".as_ref(), path.as_os_str()])
        }
        _ => Err("This kind of file cannot be summarized yet.".into()),
    }
}

pub async fn summarize(app: &AppHandle, path: &str) -> Result<String, String> {
    let path = std::path::PathBuf::from(path);
    if !path.is_file() {
        return Err("The file is gone.".into());
    }
    let handle = app.clone();
    let p = path.clone();
    let text = tauri::async_runtime::spawn_blocking(move || text_of(&handle, &p))
        .await
        .map_err(|e| e.to_string())??;
    let text: String = text.chars().take(MAX_TEXT).collect();
    if text.trim().is_empty() {
        return Err("The document has no text to read (it may be scanned).".into());
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    crate::ask::open(
        app,
        crate::ask::Open {
            prompt: Some(format!(
                "Summarize the document \"{name}\": what it is, the key points in five bullets, and anything I need to act on."
            )),
            ask: true,
            page: Some(text),
            ..Default::default()
        },
    );
    Ok(format!("Reading {name}"))
}

/// Once a day from 10:00, when Downloads has piled up: old files, loose
/// files to sort, or installers that were most likely run already. The
/// skill's cooldown keeps it to about once a week.
pub fn start_weekly_check(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut done_on = None;
        loop {
            tokio::time::sleep(Duration::from_secs(30 * 60)).await;
            let now = Local::now();
            if now.hour() < 10 || done_on == Some(now.date_naive()) {
                continue;
            }
            let allowed = {
                let state = app.state::<AppState>();
                let s = lock(&state.settings);
                !s.pause.is_active(chrono::Utc::now()) && s.sensor_enabled("downloads")
            };
            let Some(dir) = dirs::download_dir() else {
                continue;
            };
            if !allowed {
                continue;
            }
            done_on = Some(now.date_naive());
            let cutoff = SystemTime::now() - sidekick_actions::cleanup::OLD_AFTER;
            let files = sidekick_actions::cleanup::old_files(&dir, cutoff);
            let mb = files.iter().map(|(_, s)| s).sum::<u64>() / 1_000_000;
            let day = SystemTime::now() - Duration::from_secs(24 * 60 * 60);
            let loose = sidekick_actions::cleanup::loose_files(&dir, day).len();
            let installers = sidekick_actions::cleanup::old_installers(
                &dir,
                SystemTime::now() - sidekick_actions::cleanup::INSTALLER_AFTER,
            )
            .len();
            if let Some(e) = piling_up(files.len(), mb, loose, installers, &dir) {
                app.state::<AppState>().bus.publish(e);
            }
        }
    });
}

/// The Downloads event, when there is enough to bother you with.
fn piling_up(old: usize, mb: u64, loose: usize, installers: usize, dir: &Path) -> Option<Event> {
    if old < 10 && mb < 200 && loose < 25 && installers < 3 {
        return None;
    }
    let mut parts = Vec::new();
    if loose > 0 {
        parts.push(format!("{loose} loose files"));
    }
    if installers > 0 {
        parts.push(format!("{installers} old installers"));
    }
    if old > 0 {
        parts.push(format!("{old} files over 30 days ({mb} MB)"));
    }
    Some(Event::new(
        DOWNLOADS_OLD,
        "downloads",
        serde_json::json!({
            "count": old, "mb": mb, "loose": loose, "installers": installers,
            "summary": parts.join(", "), "dir": dir.display().to_string(),
        }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speaks_up_only_when_downloads_piles_up() {
        let dir = Path::new("C:/Users/me/Downloads");
        assert!(piling_up(3, 10, 5, 1, dir).is_none());
        let e = piling_up(0, 0, 30, 4, dir).unwrap();
        assert_eq!(e.payload["summary"], "30 loose files, 4 old installers");
    }
}
