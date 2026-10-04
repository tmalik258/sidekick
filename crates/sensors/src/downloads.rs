use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use notify::event::{ModifyKind, RenameMode};
use notify::{EventKind, RecursiveMode, Watcher};
use sidekick_core::{Event, EventBus, Sensitivity};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::classify::{file_kind, is_partial_or_hidden};
use crate::{Sensor, SensorGate};

/// Watches the Downloads folder through OS file notifications
/// (ReadDirectoryChangesW on Windows), so a finished download is noticed as
/// soon as its size settles, not on a timer.
///
/// The same watcher also serves the Screenshots folder, with its
/// own sensor id and event kind.
pub struct DownloadsSensor {
    dir: Option<PathBuf>,
    id: &'static str,
    kind: &'static str,
}

impl DownloadsSensor {
    pub const ID: &'static str = "downloads";
    pub const EVENT_KIND: &'static str = "file.download_completed";
    pub const SCREENSHOTS_ID: &'static str = "screenshots";
    pub const SCREENSHOT_KIND: &'static str = "file.screenshot";

    /// Watches the user's Downloads folder.
    pub fn new() -> Self {
        Self {
            dir: dirs::download_dir(),
            id: Self::ID,
            kind: Self::EVENT_KIND,
        }
    }

    pub fn with_dir(dir: PathBuf) -> Self {
        Self {
            dir: Some(dir),
            ..Self::new()
        }
    }

    /// Watches Pictures\Screenshots, where Win+PrtScn and the Snipping Tool
    /// save. The folder is created if it does not exist yet.
    pub fn screenshots() -> Self {
        let dir = dirs::picture_dir().map(|p| p.join("Screenshots"));
        if let Some(d) = &dir {
            let _ = std::fs::create_dir_all(d);
        }
        Self {
            dir,
            id: Self::SCREENSHOTS_ID,
            kind: Self::SCREENSHOT_KIND,
        }
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
/// A file counts as just written when its modified time is at most this old
/// on this PC's clock. OneDrive hydrating or re-stamping an older file keeps
/// its original modified time, so it stays quiet.
const FRESH: Duration = Duration::from_secs(120);
/// Clock skew allowed for a modified time ahead of this PC's clock; anything
/// further ahead was stamped on another machine.
const AHEAD: Duration = Duration::from_secs(5);

impl Sensor for DownloadsSensor {
    fn id(&self) -> &'static str {
        self.id
    }

    fn spawn(self: Box<Self>, bus: EventBus, gate: SensorGate) -> JoinHandle<()> {
        let (id, kind) = (self.id, self.kind);
        tokio::spawn(async move {
            let Some(dir) = self.dir else {
                log::warn!("{id} sensor: no folder found");
                return;
            };
            // (path, renamed from a partial download)
            let (tx, mut rx) = mpsc::unbounded_channel::<(PathBuf, bool)>();
            // Windows reports a rename as From then To, as two events.
            let mut from_partial = false;
            let mut watcher =
                match notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                    let Ok(event) = res else { return };
                    match event.kind {
                        EventKind::Modify(ModifyKind::Name(RenameMode::From)) => {
                            from_partial = event.paths.iter().any(|p| is_partial_or_hidden(p));
                        }
                        EventKind::Modify(ModifyKind::Name(RenameMode::To)) => {
                            for path in event.paths {
                                let _ = tx.send((path, from_partial));
                            }
                            from_partial = false;
                        }
                        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) => {
                            if let [from, to] = event.paths.as_slice() {
                                let _ = tx.send((to.clone(), is_partial_or_hidden(from)));
                            }
                        }
                        EventKind::Create(_) | EventKind::Modify(_) => {
                            for path in event.paths {
                                let _ = tx.send((path, false));
                            }
                        }
                        _ => {}
                    }
                }) {
                    Ok(w) => w,
                    Err(err) => {
                        log::error!("{id} sensor: cannot create watcher: {err}");
                        return;
                    }
                };
            if let Err(err) = watcher.watch(&dir, RecursiveMode::NonRecursive) {
                log::error!("{id} sensor: cannot watch {}: {err}", dir.display());
                return;
            }
            log::info!("{id} sensor watching {}", dir.display());

            // path -> (last seen size, when it last changed, renamed from a
            // partial download)
            let mut pending: HashMap<PathBuf, (u64, Instant, bool)> = HashMap::new();
            // path -> modified time it was reported with, so a sync touch on
            // the same contents is skipped but a same-name overwrite is not.
            let mut reported: HashMap<PathBuf, SystemTime> = HashMap::new();
            let mut tick = tokio::time::interval(CHECK_EVERY);

            loop {
                tokio::select! {
                    Some((path, renamed)) = rx.recv() => {
                        if !is_partial_or_hidden(&path) {
                            let renamed = renamed || pending.get(&path).is_some_and(|p| p.2);
                            pending.insert(path, (u64::MAX, Instant::now(), renamed));
                        }
                    }
                    _ = tick.tick() => {
                        if pending.is_empty() {
                            continue;
                        }
                        let now = Instant::now();
                        let wall = SystemTime::now();
                        reported.retain(|_, at| !matches!(wall.duration_since(*at), Ok(age) if age >= FRESH));
                        let mut done = Vec::new();
                        for (path, (size, since, renamed)) in pending.iter_mut() {
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
                                if meta.len() > 0
                                    && let Some(modified) = written_now(&meta, wall, *renamed)
                                    && reported.get(path) != Some(&modified)
                                    && gate.allows(id)
                                {
                                    reported.insert(path.clone(), modified);
                                    let event = file_event(id, kind, path, meta.len());
                                    if kind == Self::EVENT_KIND {
                                        // Hashing and signature checks take a moment.
                                        let (bus, path) = (bus.clone(), path.clone());
                                        tokio::spawn(async move {
                                            let event = tokio::task::spawn_blocking(move || enrich(event, &path))
                                                .await;
                                            if let Ok(e) = event {
                                                bus.publish(e);
                                            }
                                        });
                                    } else {
                                        bus.publish(event);
                                    }
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

/// The modified time to report a settled file under, or None when it was not
/// just written on this PC (an old file being synced, a time stamped ahead
/// elsewhere, or an OneDrive cloud placeholder). A file a browser just renamed
/// from its partial download always counts, since Firefox can give it the
/// server's date.
fn written_now(
    meta: &std::fs::Metadata,
    now: SystemTime,
    renamed_from_partial: bool,
) -> Option<SystemTime> {
    let modified = meta.modified().ok()?;
    if renamed_from_partial {
        return Some(modified);
    }
    if is_cloud_placeholder(meta) {
        return None;
    }
    let fresh = match now.duration_since(modified) {
        Ok(age) => age <= FRESH,
        Err(ahead) => ahead.duration() <= AHEAD,
    };
    fresh.then_some(modified)
}

/// Files On-Demand placeholders whose contents still live in the cloud.
#[cfg(windows)]
fn is_cloud_placeholder(meta: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const OFFLINE: u32 = 0x1000;
    const RECALL_ON_OPEN: u32 = 0x4_0000;
    const RECALL_ON_DATA_ACCESS: u32 = 0x40_0000;
    meta.file_attributes() & (OFFLINE | RECALL_ON_OPEN | RECALL_ON_DATA_ACCESS) != 0
}

#[cfg(not(windows))]
fn is_cloud_placeholder(_meta: &std::fs::Metadata) -> bool {
    false
}

/// Adds `duplicate_of` and, for installers, `signature`.
fn enrich(mut event: Event, path: &Path) -> Event {
    if let Some(dup) = crate::file_info::duplicate_of(path) {
        event.payload["duplicate_of"] = dup.display().to_string().into();
        event.payload["duplicate_name"] = dup
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
            .into();
    }
    if event.payload["kind"] == "installer" {
        event.payload["signature"] = crate::file_info::signature(path).into();
    }
    event
}

fn file_event(id: &'static str, kind: &'static str, path: &Path, size: u64) -> Event {
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
        kind,
        id,
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

    #[test]
    fn only_a_modified_time_from_just_now_on_this_clock_counts() {
        let path = std::env::temp_dir().join(format!("sidekick-fresh-{}.txt", std::process::id()));
        std::fs::write(&path, b"hello").unwrap();
        let meta = std::fs::metadata(&path).unwrap();
        let modified = meta.modified().unwrap();

        assert_eq!(written_now(&meta, modified, false), Some(modified));
        let old = modified + FRESH + Duration::from_secs(1);
        assert_eq!(
            written_now(&meta, old, false),
            None,
            "an old file being synced"
        );
        let ahead = modified - AHEAD - Duration::from_secs(1);
        assert_eq!(
            written_now(&meta, ahead, false),
            None,
            "stamped ahead on another PC"
        );
        assert_eq!(
            written_now(&meta, old, true),
            Some(modified),
            "a browser finished it here"
        );
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn skips_an_old_synced_file_but_reports_a_same_name_overwrite() {
        let dir = std::env::temp_dir().join(format!("sidekick-sync-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bus = EventBus::default();
        let mut rx = bus.subscribe();
        let (_handle, gate) = SensorGate::new(GateState::default());
        let task = Box::new(DownloadsSensor::with_dir(dir.clone())).spawn(bus, gate);
        tokio::time::sleep(Duration::from_millis(200)).await;

        let path = dir.join("Screenshot 2026-04-06 231002.png");
        std::fs::write(&path, b"png bytes").unwrap();
        let file = std::fs::File::options().write(true).open(&path).unwrap();
        file.set_modified(SystemTime::now() - Duration::from_secs(180 * 24 * 3600))
            .unwrap();
        drop(file);
        assert!(
            tokio::time::timeout(Duration::from_millis(1500), rx.recv())
                .await
                .is_err(),
            "an old file touched by sync is not announced"
        );

        std::fs::write(&path, b"png bytes").unwrap();
        let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("overwrite event in time")
            .unwrap();
        assert_eq!(event.payload["name"], "Screenshot 2026-04-06 231002.png");
        task.abort();
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Writes a Firefox-style partial download dated like the server's file.
    fn old_partial(path: &Path) {
        std::fs::write(path, b"%PDF-1.7 server copy").unwrap();
        let file = std::fs::File::options().write(true).open(path).unwrap();
        file.set_modified(SystemTime::now() - Duration::from_secs(365 * 24 * 3600))
            .unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn reports_a_finished_download_dated_by_the_server_even_over_the_same_name() {
        let dir = std::env::temp_dir().join(format!("sidekick-ff-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bus = EventBus::default();
        let mut rx = bus.subscribe();
        let (_handle, gate) = SensorGate::new(GateState::default());
        let task = Box::new(DownloadsSensor::with_dir(dir.clone())).spawn(bus, gate);
        tokio::time::sleep(Duration::from_millis(200)).await;

        let (part, done) = (dir.join("report.pdf.part"), dir.join("report.pdf"));
        for round in ["new file", "same-name replace"] {
            old_partial(&part);
            std::fs::rename(&part, &done).unwrap();
            let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
                .await
                .unwrap_or_else(|_| panic!("{round}: event in time"))
                .unwrap();
            assert_eq!(event.payload["name"], "report.pdf", "{round}");
        }
        task.abort();
        std::fs::remove_dir_all(dir).unwrap();
    }
}
