use std::time::Duration;

use sidekick_core::{Event, EventBus};
use tokio::task::JoinHandle;

use crate::{Sensor, SensorGate};

/// Notices when the user steps away and comes back, from the
/// time since the last keyboard or mouse input. Suggestions wait while the
/// user is away instead of popping up to an empty room.
pub struct IdleSensor {
    pub away_after: Duration,
}

impl IdleSensor {
    pub const ID: &'static str = "idle";
    pub const IDLE: &'static str = "user.idle";
    pub const ACTIVE: &'static str = "user.active";
}

impl Default for IdleSensor {
    fn default() -> Self {
        Self {
            away_after: Duration::from_secs(5 * 60),
        }
    }
}

const CHECK_EVERY: Duration = Duration::from_secs(2);

impl Sensor for IdleSensor {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn spawn(self: Box<Self>, bus: EventBus, gate: SensorGate) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut away: Option<Duration> = None;
            let mut tick = tokio::time::interval(CHECK_EVERY);
            loop {
                tick.tick().await;
                let Some(idle) = idle_time() else {
                    // Not supported on this OS; nothing to watch.
                    return;
                };
                if !gate.allows(Self::ID) {
                    continue;
                }
                match away {
                    None if idle >= self.away_after => {
                        away = Some(idle);
                        bus.publish(Event::new(
                            Self::IDLE,
                            Self::ID,
                            serde_json::json!({ "idle_secs": idle.as_secs() }),
                        ));
                    }
                    Some(longest) if idle < CHECK_EVERY * 2 => {
                        away = None;
                        bus.publish(Event::new(
                            Self::ACTIVE,
                            Self::ID,
                            serde_json::json!({ "away_secs": longest.as_secs() }),
                        ));
                    }
                    Some(longest) if idle > longest => away = Some(idle),
                    _ => {}
                }
            }
        })
    }
}

/// Time since the last keyboard or mouse input.
#[cfg(windows)]
pub fn idle_time() -> Option<Duration> {
    use windows_sys::Win32::System::SystemInformation::GetTickCount;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    let mut info = LASTINPUTINFO {
        cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    // SAFETY: plain query with a correctly sized out-parameter.
    let ok = unsafe { GetLastInputInfo(&mut info) } != 0;
    if !ok {
        return None;
    }
    // SAFETY: no arguments; returns milliseconds since boot (wraps at 49 days,
    // which wrapping_sub handles).
    let now = unsafe { GetTickCount() };
    Some(Duration::from_millis(u64::from(
        now.wrapping_sub(info.dwTime),
    )))
}

#[cfg(not(windows))]
pub fn idle_time() -> Option<Duration> {
    None
}
