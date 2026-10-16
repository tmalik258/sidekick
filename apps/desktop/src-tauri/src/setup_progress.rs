//! Captures only output from setup commands Duskie generated. The terminal
//! remains visible; a launched terminal is never reported as an installation.
use crate::activity::{self, Phase};
use crate::state::{AppState, lock};
use std::collections::BTreeMap;
use std::io::{Read, Seek, SeekFrom};
use std::sync::{Arc, LazyLock, Mutex};
use tauri::{AppHandle, Manager};
static MONITORS: Mutex<BTreeMap<String, Arc<tokio::sync::Notify>>> = Mutex::new(BTreeMap::new());
fn substantive_line(raw: &str) -> Option<String> {
    static ANSI: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"\x1b\[[0-?]*[ -/]*[@-~]").unwrap());
    let line = ANSI.replace_all(raw, "").trim().to_owned();
    if line.is_empty()
        || line
            .chars()
            .all(|c| "-/\\|. ".contains(c) || ('\u{2800}'..='\u{28ff}').contains(&c))
    {
        None
    } else {
        Some(line)
    }
}
pub fn check(id: &str) {
    if let Some(monitor) = lock(&MONITORS).get(id) {
        monitor.notify_one();
    }
}

#[derive(Default)]
pub struct Layers(BTreeMap<String, (u64, u64)>);
impl Layers {
    pub fn update(&mut self, value: &serde_json::Value) -> (u64, Option<u64>) {
        if let Some(digest) = value["digest"].as_str() {
            let layer = self.0.entry(digest.into()).or_default();
            layer.0 = layer.0.max(value["completed"].as_u64().unwrap_or(0));
            layer.1 = layer.1.max(value["total"].as_u64().unwrap_or(0));
        }
        let done = self.0.values().map(|l| l.0).sum();
        // Ollama reveals layers incrementally; their combined size is not the
        // whole model's known total until the stream confirms success.
        let total = (value["status"] == "success").then(|| self.0.values().map(|l| l.1).sum());
        (done, total)
    }
}

pub fn prepare(
    app: &AppHandle,
    label: &str,
    command: &str,
    queued: bool,
) -> Result<String, String> {
    let id = format!("setup:{}", ulid::Ulid::new());
    let dir = app.state::<AppState>().data_dir.join("setup-output");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{}.log", id.replace(':', "-")));
    std::fs::write(&path, b"").map_err(|e| e.to_string())?;
    let quoted_path = path.to_string_lossy().replace('\'', "''");
    let base = lock(&app.state::<AppState>().settings)
        .ai
        .local
        .base_url
        .clone();
    // Stream the model pull API through curl in the visible terminal. This
    // avoids scraping animated CLI percentages and keeps per-layer digests.
    let pulls = regex::Regex::new(r"ollama pull ([A-Za-z0-9_.:/-]+)").unwrap();
    let command = pulls.replace_all(command, |captures: &regex::Captures<'_>| {
        let body = serde_json::json!({"model": &captures[1], "stream": true}).to_string();
        let endpoint = format!("{}/api/pull", duskie_ai::loopback(base.trim_end_matches('/')));
        // Windows PowerShell's native argument passing loses JSON quotes;
        // send the JSON request on stdin instead.
        format!("'{}' | curl.exe --no-buffer --silent --show-error --fail -H 'Content-Type: application/json' --data-binary '@-' '{}'", body.replace('\'', "''"), endpoint.replace('\'', "''"))
    });
    let script = format!(
        "$duskieLog='{quoted_path}'; $duskieExit=0; $duskieFinished=$false; [IO.File]::AppendAllText($duskieLog, ('DUSKIE_START:{id}' + [Environment]::NewLine), [Text.UTF8Encoding]::new($false)); $ErrorActionPreference='Continue'; $global:LASTEXITCODE=0; try {{ & {{ {command} }} 2>&1 | ForEach-Object {{ if ($_ -is [System.Management.Automation.ErrorRecord]) {{ if ($_.FullyQualifiedErrorId -match '^NativeCommandError') {{ $_.Exception.Message }} else {{ $duskieExit=1; $_ }} }} else {{ $_ }} }} | Out-String -Stream | ForEach-Object {{ [IO.File]::AppendAllText($duskieLog, ([string]$_ + [Environment]::NewLine), [Text.UTF8Encoding]::new($false)); Write-Host $_ }}; if ($LASTEXITCODE) {{ $duskieExit=$LASTEXITCODE }}; $duskieFinished=$true }} catch {{ $duskieExit=1; [IO.File]::AppendAllText($duskieLog, ($_ | Out-String), [Text.UTF8Encoding]::new($false)); Write-Host $_ -ForegroundColor Red }} finally {{ if (-not $duskieFinished -and $duskieExit -eq 0) {{ $duskieExit=1 }}; [IO.File]::AppendAllText($duskieLog, ('DUSKIE_RESULT:{id}:' + $duskieExit + [Environment]::NewLine), [Text.UTF8Encoding]::new($false)) }}"
    );
    activity::report(app, &id, "setup", label, Phase::Running, None, None, None);
    // Only the step actually starting takes the headline. Its start/output
    // record reveals it; assembly or launch failures reveal terminal outcomes.
    if queued {
        activity::activity_dismiss(app.clone(), id.clone());
    }
    let signal = Arc::new(tokio::sync::Notify::new());
    lock(&MONITORS).insert(id.clone(), signal.clone());
    let (app, label) = (app.clone(), label.to_owned());
    tauri::async_runtime::spawn(async move {
        let mut offset = 0;
        let mut buffered = String::new();
        let mut layers = Layers::default();
        let mut phase = Phase::Running;
        loop {
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_millis(500)) => {},
                _ = signal.notified() => {},
            }
            let Ok(mut file) = std::fs::File::open(&path) else {
                activity::report(
                    &app,
                    &id,
                    "setup",
                    &label,
                    Phase::Unconfirmed,
                    None,
                    None,
                    Some("Setup output is unavailable".into()),
                );
                continue;
            };
            if file.seek(SeekFrom::Start(offset)).is_err() {
                activity::report(
                    &app,
                    &id,
                    "setup",
                    &label,
                    Phase::Unconfirmed,
                    None,
                    None,
                    Some("Setup output is unavailable".into()),
                );
                continue;
            }
            let mut bytes = Vec::new();
            if file.take(64 * 1024).read_to_end(&mut bytes).is_err() {
                continue;
            }
            offset += bytes.len() as u64;
            buffered.push_str(&String::from_utf8_lossy(&bytes));
            while let Some(end) = buffered.find('\n') {
                let line = substantive_line(&buffered[..end]);
                buffered.drain(..=end);
                let Some(line) = line else {
                    continue;
                };
                if line == format!("DUSKIE_START:{id}") {
                    activity::report(
                        &app,
                        &id,
                        "setup",
                        &label,
                        Phase::Running,
                        None,
                        None,
                        Some("Setup step started".into()),
                    );
                    continue;
                }
                if let Some(code) = line.strip_prefix(&format!("DUSKIE_RESULT:{id}:")) {
                    activity::report(
                        &app,
                        &id,
                        "setup",
                        &label,
                        if code == "0" {
                            Phase::Completed
                        } else {
                            Phase::Failed
                        },
                        None,
                        None,
                        Some(if code == "0" {
                            "Setup command finished".into()
                        } else {
                            format!("Setup command failed (exit {code})")
                        }),
                    );
                    let _ = std::fs::remove_file(&path);
                    lock(&MONITORS).remove(&id);
                    return;
                }
                if line.is_empty() {
                    continue;
                }
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) {
                    if let Some(error) = value["error"].as_str() {
                        activity::report(
                            &app,
                            &id,
                            "setup",
                            &label,
                            Phase::Failed,
                            None,
                            None,
                            Some(error.into()),
                        );
                        continue;
                    }
                    let status = value["status"].as_str().unwrap_or_default();
                    let (done, total) = layers.update(&value);
                    phase = if status.contains("verifying") {
                        Phase::Verifying
                    } else if status.contains("pulling") {
                        Phase::Downloading
                    } else {
                        Phase::Installing
                    };
                    activity::report(
                        &app,
                        &id,
                        "setup",
                        &label,
                        phase.clone(),
                        Some(done),
                        total,
                        Some(status.into()),
                    );
                } else {
                    let lower = line.to_ascii_lowercase();
                    if lower.contains("downloading") {
                        phase = Phase::Downloading;
                    } else if lower.contains("installing") {
                        phase = Phase::Installing;
                    } else if lower.contains("verifying") {
                        phase = Phase::Verifying;
                    }
                    activity::report(
                        &app,
                        &id,
                        "setup",
                        &label,
                        phase.clone(),
                        None,
                        None,
                        Some(line.chars().take(512).collect()),
                    );
                }
            }
        }
    });
    Ok(script)
}

pub fn launch_failed(script: &str, error: &str) {
    let logs = regex::Regex::new(r"\$duskieLog='((?:[^']|'')*)'").unwrap();
    for capture in logs.captures_iter(script) {
        let path = std::path::PathBuf::from(capture[1].replace("''", "'"));
        // Only the private logs emitted by prepare; no inferred processes.
        if path
            .parent()
            .and_then(|p| p.file_name())
            .is_some_and(|p| p == "setup-output")
        {
            let id = path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .replacen("setup-", "setup:", 1);
            let _ = std::fs::write(path, format!("{error}\nDUSKIE_RESULT:{id}:1\n"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn spinner_heartbeats_are_not_substantive_output() {
        for line in ["|", "/", "\\", "-", "...", "\x1b[33m⠋\x1b[0m"] {
            assert!(substantive_line(line).is_none());
        }
        assert_eq!(
            substantive_line("\x1b[33mDownloading package\x1b[0m"),
            Some("Downloading package".into())
        );
    }
    #[test]
    fn layers_are_aggregated_and_repeated_reports_do_not_double_count() {
        let mut layers = Layers::default();
        let layer = serde_json::json!({"digest":"a", "completed":10, "total":20});
        assert_eq!(layers.update(&layer), (10, None));
        assert_eq!(layers.update(&layer), (10, None));
        assert_eq!(
            layers.update(&serde_json::json!({"digest":"b", "completed":30, "total":30})),
            (40, None)
        );
        assert_eq!(
            layers.update(&serde_json::json!({"status":"success"})),
            (40, Some(50))
        );
    }
}
