use std::collections::HashSet;
use std::time::Duration;

use sidekick_core::{Event, EventBus};
use sysinfo::{
    Disks, MemoryRefreshKind, ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System,
};
use tokio::task::JoinHandle;

use crate::{Sensor, SensorGate};

/// Disk space and memory pressure (FR-SYS-01, FR-SYS-02).
pub struct SystemSensor;

impl SystemSensor {
    pub const ID: &'static str = "system";
    pub const DISK_LOW: &'static str = "system.disk_low";
    pub const MEMORY_HIGH: &'static str = "system.memory_high";
}

const CHECK_EVERY: Duration = Duration::from_secs(15);
/// Disks are checked every fourth memory check (once a minute).
const DISK_EVERY_N: u32 = 4;
/// Memory must stay high this many checks in a row (45 s).
const MEMORY_STREAK: u32 = 3;
const MEMORY_HIGH_PCT: f64 = 92.0;
const MEMORY_CLEAR_PCT: f64 = 85.0;
const GB: u64 = 1024 * 1024 * 1024;

/// A drive is low below 10 GB or 8 percent free, whichever is larger.
pub fn disk_is_low(free: u64, total: u64) -> bool {
    total >= 20 * GB && free < (10 * GB).max(total * 8 / 100)
}

/// Back to normal once 2 GB above the line, so it does not flap.
fn disk_recovered(free: u64, total: u64) -> bool {
    free >= (10 * GB).max(total * 8 / 100) + 2 * GB
}

fn human(bytes: u64) -> String {
    let gb = bytes as f64 / GB as f64;
    if gb >= 10.0 {
        format!("{gb:.0} GB")
    } else {
        format!("{gb:.1} GB")
    }
}

impl Sensor for SystemSensor {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn spawn(self: Box<Self>, bus: EventBus, gate: SensorGate) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(CHECK_EVERY);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            let mut n: u32 = 0;
            let mut low_disks: HashSet<String> = HashSet::new();
            let mut streak: u32 = 0;
            let mut memory_reported = false;
            loop {
                tick.tick().await;
                n = n.wrapping_add(1);
                let check_disks = n % DISK_EVERY_N == 1;
                let snap = tokio::task::spawn_blocking(move || snapshot(check_disks))
                    .await
                    .unwrap_or_default();
                if !gate.allows(Self::ID) {
                    continue;
                }

                for d in &snap.disks {
                    if disk_is_low(d.free, d.total) {
                        if low_disks.insert(d.mount.clone()) {
                            bus.publish(Event::new(
                                Self::DISK_LOW,
                                Self::ID,
                                serde_json::json!({
                                    "mount": d.mount,
                                    "free": d.free,
                                    "total": d.total,
                                    "free_human": human(d.free),
                                    "total_human": human(d.total),
                                    "percent_free": (d.free as f64 * 100.0 / d.total as f64).round(),
                                }),
                            ));
                        }
                    } else if disk_recovered(d.free, d.total) {
                        low_disks.remove(&d.mount);
                    }
                }

                if let Some(mem) = snap.memory {
                    if mem.percent >= MEMORY_HIGH_PCT {
                        streak += 1;
                    } else {
                        streak = 0;
                    }
                    if mem.percent < MEMORY_CLEAR_PCT {
                        memory_reported = false;
                    }
                    if streak >= MEMORY_STREAK && !memory_reported {
                        memory_reported = true;
                        bus.publish(Event::new(
                            Self::MEMORY_HIGH,
                            Self::ID,
                            serde_json::json!({
                                "percent": mem.percent.round(),
                                "process": mem.top_name,
                                "process_mb": mem.top_bytes / (1024 * 1024),
                                "pid": mem.top_pid,
                            }),
                        ));
                    }
                }
            }
        })
    }
}

#[derive(Default)]
struct Snapshot {
    disks: Vec<DiskInfo>,
    memory: Option<MemoryInfo>,
}

struct DiskInfo {
    mount: String,
    free: u64,
    total: u64,
}

struct MemoryInfo {
    percent: f64,
    top_name: String,
    top_bytes: u64,
    top_pid: u32,
}

fn snapshot(check_disks: bool) -> Snapshot {
    let mut snap = Snapshot::default();
    if check_disks {
        let mut seen = HashSet::new();
        snap.disks = Disks::new_with_refreshed_list()
            .iter()
            .filter(|d| !d.is_removable())
            .filter_map(|d| {
                let mount = d.mount_point().to_string_lossy().into_owned();
                seen.insert(mount.clone()).then_some(DiskInfo {
                    mount,
                    free: d.available_space(),
                    total: d.total_space(),
                })
            })
            .collect();
    }
    let mut sys = System::new_with_specifics(
        RefreshKind::nothing().with_memory(MemoryRefreshKind::nothing().with_ram()),
    );
    let total = sys.total_memory();
    if total > 0 {
        let percent = sys.used_memory() as f64 * 100.0 / total as f64;
        let mut top = (String::new(), 0u64, 0u32);
        if percent >= MEMORY_HIGH_PCT {
            sys.refresh_processes_specifics(
                ProcessesToUpdate::All,
                true,
                ProcessRefreshKind::nothing().with_memory(),
            );
            if let Some((pid, p)) = sys.processes().iter().max_by_key(|(_, p)| p.memory()) {
                top = (
                    p.name()
                        .to_string_lossy()
                        .trim_end_matches(".exe")
                        .to_owned(),
                    p.memory(),
                    pid.as_u32(),
                );
            }
        }
        snap.memory = Some(MemoryInfo {
            percent,
            top_name: top.0,
            top_bytes: top.1,
            top_pid: top.2,
        });
    }
    snap
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn low_disk_thresholds() {
        // 256 GB drive: the line is 20.5 GB (8 percent).
        assert!(disk_is_low(15 * GB, 256 * GB));
        assert!(!disk_is_low(30 * GB, 256 * GB));
        // 60 GB drive: the line is 10 GB.
        assert!(disk_is_low(9 * GB, 60 * GB));
        // Tiny drives (USB sticks, recovery partitions) are ignored.
        assert!(!disk_is_low(GB, 16 * GB));
        // It only clears 2 GB above the line.
        assert!(!disk_recovered(11 * GB, 60 * GB));
        assert!(disk_recovered(12 * GB, 60 * GB));
    }

    #[test]
    fn sizes_read_well() {
        assert_eq!(human(8 * GB + GB / 2), "8.5 GB");
        assert_eq!(human(120 * GB), "120 GB");
    }

    #[test]
    fn snapshot_reads_this_machine() {
        let s = snapshot(true);
        assert!(
            s.memory
                .is_some_and(|m| m.percent > 0.0 && m.percent <= 100.0)
        );
    }
}
