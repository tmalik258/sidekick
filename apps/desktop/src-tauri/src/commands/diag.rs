//! Things for bug reports and how the island looks on this PC: Copy
//! diagnostics, the opt-in crash report, and whether Windows wants
//! transparency effects or is saving battery.

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use super::CmdResult;
use crate::state::{AppState, lock};

/// What the island adapts to: solid instead of glass, simpler on battery saver.
#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SystemLook {
    transparency: bool,
    battery_saver: bool,
}

#[tauri::command]
pub async fn system_look() -> SystemLook {
    // Registry and power status: off the UI thread.
    tauri::async_runtime::spawn_blocking(|| SystemLook {
        transparency: sidekick_sensors::transparency_effects(),
        battery_saver: sidekick_sensors::battery_saver(),
    })
    .await
    .unwrap_or_default()
}

const CRASH_FILE: &str = "last-crash.txt";
/// Log lines in a report.
const LOG_LINES: usize = 60;

/// Nothing personal: secrets masked, the user's home folder (and so their
/// name) replaced by `~`.
pub fn scrub(text: &str) -> String {
    let masked = sidekick_sensors::classify::mask(text).into_owned();
    match dirs::home_dir() {
        Some(home) => {
            let home = home.display().to_string();
            masked
                .replace(&home, "~")
                .replace(&home.replace('\\', "/"), "~")
                .replace(&home.replace('\\', "\\\\"), "~")
        }
        None => masked,
    }
}

fn last_lines(path: &std::path::Path, n: usize) -> String {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

fn log_file(app: &AppHandle) -> Option<std::path::PathBuf> {
    let dir = app.path().app_log_dir().ok()?;
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "log"))
        .max_by_key(|p| p.metadata().and_then(|m| m.modified()).ok())
}

/// Version, Windows, which models answer, recent timings, the last crash and
/// the end of the log, ready to paste into an issue.
#[tauri::command]
pub async fn diagnostics(app: AppHandle) -> String {
    let status = crate::ai::status(&app).await;
    let (settings, data_dir) = {
        let state = app.state::<AppState>();
        (lock(&state.settings).clone(), state.data_dir.clone())
    };
    let mut out = format!(
        "Sidekick {}\n{}\nMemory: {} GB\n",
        app.package_info().version,
        sysinfo::System::long_os_version().unwrap_or_else(|| "Unknown OS".into()),
        sysinfo::System::new_with_specifics(
            sysinfo::RefreshKind::nothing().with_memory(sysinfo::MemoryRefreshKind::everything())
        )
        .total_memory()
            / (1024 * 1024 * 1024),
    );
    out.push_str(&format!(
        "Models in order: {}\nAvailable: {}\n",
        settings.ai.order.join(", "),
        serde_json::to_string(&status).unwrap_or_default()
    ));
    out.push_str(&format!(
        "Voice: {}, Island: {}, Transparency effects: {}, Battery saver: {}\n",
        if settings.voice.enabled { "on" } else { "off" },
        settings.island_color,
        sidekick_sensors::transparency_effects(),
        sidekick_sensors::battery_saver(),
    ));
    let timings = crate::timings::recent(&app, 6);
    if !timings.is_empty() {
        let list: Vec<String> = timings
            .iter()
            .map(|t| format!("{} {} ms", t.name, t.ms))
            .collect();
        out.push_str(&format!("Timings: {}\n", list.join("; ")));
    }
    let freezes = crate::freeze::report();
    if !freezes.is_empty() {
        let list: Vec<String> = freezes
            .iter()
            .rev()
            .take(10)
            .map(|f| format!("{} {} ms at {} s", f.what, f.ms, f.at))
            .collect();
        out.push_str(&format!("UI thread held: {}\n", list.join("; ")));
    }
    if let Ok(crash) = std::fs::read_to_string(data_dir.join(CRASH_FILE)) {
        out.push_str(&format!("\nLast crash:\n{}\n", crash.trim()));
    }
    if let Some(log) = log_file(&app) {
        out.push_str(&format!(
            "\nLog (last {LOG_LINES} lines):\n{}\n",
            last_lines(&log, LOG_LINES)
        ));
    }
    scrub(&out)
}

/// The crash from the last run, when the user opted in to crash reports and
/// has not seen it yet.
#[tauri::command]
pub fn crash_pending(state: State<'_, AppState>) -> Option<String> {
    if !lock(&state.settings).crash_reports {
        return None;
    }
    let text = std::fs::read_to_string(state.data_dir.join(CRASH_FILE)).ok()?;
    let seen =
        std::fs::read_to_string(state.data_dir.join("last-crash-seen.txt")).unwrap_or_default();
    (text.trim() != seen.trim()).then(|| scrub(text.trim()))
}

/// Marks the last crash as seen, so it is offered once.
#[tauri::command]
pub fn crash_dismiss(state: State<'_, AppState>) -> CmdResult<()> {
    let text = std::fs::read_to_string(state.data_dir.join(CRASH_FILE)).unwrap_or_default();
    std::fs::write(state.data_dir.join("last-crash-seen.txt"), text).map_err(|e| e.to_string())
}

/// What held the UI thread long enough to show, oldest first.
#[tauri::command]
pub async fn freeze_report() -> Vec<crate::freeze::Freeze> {
    crate::freeze::report()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrubs_secrets_and_the_home_folder() {
        let home = dirs::home_dir().unwrap().display().to_string();
        let text =
            format!("opened {home}/projects/app with ghp_abcdefghijklmnopqrstuvwxyz0123456789AB");
        let s = scrub(&text);
        assert!(!s.contains(&home) && !s.contains("ghp_"), "{s}");
        assert!(s.starts_with("opened ~/projects/app with"));
    }
}
