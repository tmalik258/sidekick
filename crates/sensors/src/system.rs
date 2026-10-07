use std::collections::HashSet;
use std::time::Duration;

use sidekick_core::{Event, EventBus};
use sysinfo::{
    Disks, MemoryRefreshKind, ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System,
};
use tokio::task::JoinHandle;

use crate::{Sensor, SensorGate};

/// Disk space and memory pressure.
pub struct SystemSensor;

impl SystemSensor {
    pub const ID: &'static str = "system";
    pub const DISK_LOW: &'static str = "system.disk_low";
    pub const MEMORY_HIGH: &'static str = "system.memory_high";
    pub const BATTERY_LOW: &'static str = "system.battery_low";
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
            let mut battery_reported = false;
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

                match battery() {
                    Some((percent, false, saver)) if percent < BATTERY_LOW_PCT => {
                        if !battery_reported {
                            battery_reported = true;
                            bus.publish(Event::new(
                                Self::BATTERY_LOW,
                                Self::ID,
                                serde_json::json!({
                                    "percent": percent,
                                    "saver": if saver { "on" } else { "off" },
                                }),
                            ));
                        }
                    }
                    // Plugged in or charged again: the next low battery counts.
                    Some(_) => battery_reported = false,
                    None => {}
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

/// Battery saver is on: the island keeps its look simple to save power.
pub fn battery_saver() -> bool {
    battery().is_some_and(|(_, _, saver)| saver)
}

/// Windows' "Transparency effects" (Settings > Personalization > Colors).
/// When off, the island is a solid colour instead of glass.
#[cfg(windows)]
pub fn transparency_effects() -> bool {
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let key = wide(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
    let name = wide("EnableTransparency");
    let mut value: u32 = 1;
    let mut size = std::mem::size_of::<u32>() as u32;
    // SAFETY: valid NUL-terminated strings and an out buffer of `size` bytes.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&mut value as *mut u32).cast(),
            &mut size,
        )
    };
    // Missing value: Windows' default, on.
    status != 0 || value != 0
}

#[cfg(not(windows))]
pub fn transparency_effects() -> bool {
    true
}

/// Memory on the biggest graphics card, in bytes, from the display
/// adapters' driver keys. None without one (or off Windows).
#[cfg(windows)]
pub fn graphics_memory() -> Option<u64> {
    use windows_sys::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_ANY, RegGetValueW};
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let class = r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}";
    let read = |key: &[u16], name: &str| {
        let name = wide(name);
        let mut value: u64 = 0;
        let mut size = std::mem::size_of::<u64>() as u32;
        // SAFETY: valid NUL-terminated strings and an out buffer of `size` bytes.
        let status = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_ANY,
                std::ptr::null_mut(),
                (&mut value as *mut u64).cast(),
                &mut size,
            )
        };
        // A DWORD fills only the low 4 bytes of the zeroed value.
        (status == 0 && (size == 4 || size == 8)).then_some(value)
    };
    (0..16)
        .filter_map(|i| {
            let key = wide(&format!("{class}\\{i:04}"));
            read(&key, "HardwareInformation.qwMemorySize")
                .or_else(|| read(&key, "HardwareInformation.MemorySize"))
        })
        .filter(|&bytes| bytes > 0)
        .max()
}

#[cfg(not(windows))]
pub fn graphics_memory() -> Option<u64> {
    None
}

/// Installed memory, in bytes.
pub fn total_memory() -> u64 {
    System::new_with_specifics(
        RefreshKind::nothing().with_memory(MemoryRefreshKind::nothing().with_ram()),
    )
    .total_memory()
}

/// Below this, on battery, power saving is offered.
const BATTERY_LOW_PCT: u8 = 20;

/// Battery percent, whether it is on mains power and whether Battery
/// saver is on; None without a battery.
#[cfg(windows)]
fn battery() -> Option<(u8, bool, bool)> {
    use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    let mut s: SYSTEM_POWER_STATUS = unsafe { std::mem::zeroed() };
    // SAFETY: a valid out pointer to a zeroed struct.
    if unsafe { GetSystemPowerStatus(&mut s) } == 0 {
        return None;
    }
    // 128: no system battery; 255: unknown percent.
    if s.BatteryFlag & 128 != 0 || s.BatteryLifePercent > 100 {
        return None;
    }
    Some((
        s.BatteryLifePercent,
        s.ACLineStatus == 1,
        s.SystemStatusFlag == 1,
    ))
}

#[cfg(not(windows))]
fn battery() -> Option<(u8, bool, bool)> {
    None
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
