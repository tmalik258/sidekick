use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sidekick_core::{EventBus, MascotMachine, Settings, Storage};
use sidekick_sensors::{GateState, SensorGateHandle};

pub struct AppState {
    pub settings: Mutex<Settings>,
    pub settings_path: PathBuf,
    pub storage: Arc<Mutex<Storage>>,
    pub db_path: PathBuf,
    pub bus: EventBus,
    pub gate: SensorGateHandle,
    pub mascot: Mutex<MascotMachine>,
    /// Bumped on every mascot transition so delayed follow-ups can tell
    /// whether the state they were scheduled for is still current.
    pub mascot_epoch: AtomicU64,
    pub hit_rect: Mutex<HitRect>,
    pub suggestion: Mutex<Option<Suggestion>>,
}

/// The interactive part of the island window, in logical pixels relative to
/// the window. Everything outside it is click-through.
#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HitRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl HitRect {
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x <= self.x + self.width && y >= self.y && y <= self.y + self.height
    }
}

/// A suggestion shown as chips on the island.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    pub id: String,
    pub title: String,
    pub detail: String,
    pub options: Vec<String>,
}

pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

pub fn gate_state(settings: &Settings, now: DateTime<Utc>) -> GateState {
    GateState {
        paused: settings.pause.is_active(now),
        disabled: settings
            .sensors
            .iter()
            .filter(|(_, enabled)| !**enabled)
            .map(|(id, _)| id.clone())
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sidekick_core::Pause;

    #[test]
    fn hit_rect_contains_edges() {
        let r = HitRect {
            x: 10.0,
            y: 0.0,
            width: 100.0,
            height: 40.0,
        };
        assert!(r.contains(10.0, 0.0));
        assert!(r.contains(110.0, 40.0));
        assert!(!r.contains(9.9, 10.0));
        assert!(!r.contains(50.0, 40.1));
    }

    #[test]
    fn gate_state_reflects_pause_and_disabled_sensors() {
        let now = Utc::now();
        let mut s = Settings::default();
        s.sensors.insert("heartbeat".into(), false);
        s.sensors.insert("files".into(), true);
        s.pause = Pause::for_minutes(5, now);
        let g = gate_state(&s, now);
        assert!(g.paused);
        assert_eq!(g.disabled.into_iter().collect::<Vec<_>>(), ["heartbeat"]);
    }
}
