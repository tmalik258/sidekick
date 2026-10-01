use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use notify::{EventKind, RecursiveMode, Watcher};
use sidekick_core::{Event, EventBus, Sensitivity};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::classify::{file_kind, is_partial_or_hidden};
use crate::{Sensor, SensorGate};

/// Watches the Downloads folder through OS file notifications
/// (ReadDirectoryChangesW on Windows), so a finished download is noticed as
/// soon as its size settles, not on a timer (FR-SEN-01, FR-SEN-02).
pub struct DownloadsSensor {
    dir: Option<PathBuf>,
}

impl DownloadsSensor {
    pub const ID: &'static str = "downloads";
    pub const EVENT_KIND: &'static str = "file.download_completed";

    /// Watches the user's Downloads folder.
    pub fn new() -> Self {
        Self {
            dir: dirs::download_dir(),
        }
    }

    pub fn with_dir(dir: PathBuf) -> Self {
        Self { dir: Some(dir) }
    }
}

impl Default for DownloadsSensor {
    fn default() -> Self {
        Self::new()
    }
}

/// A file is complete once its size has not changed for this long.
const SETTLE: Duration = Duration::from_millis(700);
const CHECK_EVERY: Duration = Duration::from_millis(150);
/// The same path is not reported twice within this window.
const REPEAT_WINDOW: Duration = Duration::from_secs(15);

impl Sensor for DownloadsSensor {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn spawn(self: Box<Self>, bus: EventBus, gate: SensorGate) -> JoinHandle<()> {
        tokio::spawn(async move {
            let Some(dir) = self.dir else {
                log::warn!("downloads sensor: no Downloads folder found");
                return;
            };
            let (tx, mut rx) = mpsc::unbounded_channel::<PathBuf>();
            let mut watcher =
                match notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                    if let Ok(event) = res
                        && matches!(event.kind, EventKind::Create(_) | EventKind::Modify(_))
                    {
                        for path in event.paths {
                            let _ = tx.send(path);
                        }
                    }
                }) {
                    Ok(w) => w,
                    Err(err) => {
                        log::error!("downloads sensor: cannot create watcher: {err}");
                        return;
                    }
                };
            if let Err(err) = watcher.watch(&dir, RecursiveMode::NonRecursive) {
                log::error!("downloads sensor: cannot watch {}: {err}", dir.display());
                return;
            }
            log::info!("downloads sensor watching {}", dir.display());

            // path -> (last seen size, when it last changed)
            let mut pending: HashMap<PathBuf, (u64, Instant)> = HashMap::new();
            let mut reported: HashMap<PathBuf, Instant> = HashMap::new();
            let mut tick = tokio::time::interval(CHECK_EVERY);

            loop {
                tokio::select! {
                    Some(path) = rx.recv() => {
                        if !is_partial_or_hidden(&path) {
                            pending.insert(path, (u64::MAX, Instant::now()));
                        }
                    }
                    _ = tick.tick() => {
                        if pending.is_empty() {
                            continue;
                        }
                        let now = Instant::now();
                        reported.retain(|_, at| now.duration_since(*at) < REPEAT_WINDOW);
                        let mut done = Vec::new();
                        for (path, (size, since)) in pending.iter_mut() {
                            let Ok(meta) = std::fs::metadata(path) else {
                                done.push(path.clone());
                                continue;
                            };
                            if !meta.is_file() {
                                done.push(path.clone());
                            } else if meta.len() != *size {
                                *size = meta.len();
                                *since = now;
                            } else if now.duration_since(*since) >= SETTLE {
                                done.push(path.clone());
                                if meta.len() > 0 && !reported.contains_key(path) && gate.allows(Self::ID) {
                                    reported.insert(path.clone(), now);
                                    bus.publish(download_event(path, meta.len()));
                                }
                            }
                        }
                        for p in done {
                            pending.remove(&p);
                        }
                    }
                }
            }
        })
    }
}

fn download_event(path: &Path, size: u64) -> Event {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    Event::new(
        DownloadsSensor::EVENT_KIND,
        DownloadsSensor::ID,
        serde_json::json!({
            "path": path.display().to_string(),
            "dir": path.parent().map(|p| p.display().to_string()).unwrap_or_default(),
            "name": name,
            "ext": ext,
            "kind": file_kind(path),
            "size": size,
        }),
    )
    .with_sensitivity(Sensitivity::Personal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GateState;

    #[tokio::test(flavor = "multi_thread")]
    async fn reports_a_finished_download_once_and_skips_partials() {
        let dir = std::env::temp_dir().join(format!("sidekick-dl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bus = EventBus::default();
        let mut rx = bus.subscribe();
        let (_handle, gate) = SensorGate::new(GateState::default());
        let task = Box::new(DownloadsSensor::with_dir(dir.clone())).spawn(bus, gate);
        tokio::time::sleep(Duration::from_millis(200)).await;

        std::fs::write(dir.join("movie.mp4.crdownload"), b"partial").unwrap();
        std::fs::write(dir.join("invoice.pdf"), b"%PDF-1.7 hello").unwrap();

        let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("download event in time")
            .unwrap();
        assert_eq!(event.kind, DownloadsSensor::EVENT_KIND);
        assert_eq!(event.payload["name"], "invoice.pdf");
        assert_eq!(event.payload["kind"], "document");
        assert!(
            tokio::time::timeout(Duration::from_millis(1500), rx.recv())
                .await
                .is_err(),
            "no second event for the same file or the partial"
        );
        task.abort();
        std::fs::remove_dir_all(dir).unwrap();
    }
}
