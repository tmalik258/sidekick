use std::collections::BTreeSet;

use tokio::sync::watch;

/// What sensors are allowed to do right now.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GateState {
    pub paused: bool,
    pub disabled: BTreeSet<String>,
}

/// Read side, cloned into every sensor.
#[derive(Clone)]
pub struct SensorGate {
    rx: watch::Receiver<GateState>,
}

/// Write side, kept by the app.
pub struct SensorGateHandle {
    tx: watch::Sender<GateState>,
}

impl SensorGate {
    pub fn new(initial: GateState) -> (SensorGateHandle, SensorGate) {
        let (tx, rx) = watch::channel(initial);
        (SensorGateHandle { tx }, SensorGate { rx })
    }

    /// True when the sensor may emit events.
    pub fn allows(&self, sensor_id: &str) -> bool {
        let state = self.rx.borrow();
        !state.paused && !state.disabled.contains(sensor_id)
    }

    /// Waits until the gate state changes. Returns false when the app dropped
    /// the handle, which means the sensor should stop.
    pub async fn changed(&mut self) -> bool {
        self.rx.changed().await.is_ok()
    }
}

impl SensorGateHandle {
    pub fn set(&self, state: GateState) {
        self.tx.send_replace(state);
    }

    pub fn state(&self) -> GateState {
        self.tx.borrow().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn respects_pause_and_disabled_sensors() {
        let (handle, gate) = SensorGate::new(GateState::default());
        assert!(gate.allows("heartbeat"));

        handle.set(GateState {
            paused: true,
            ..GateState::default()
        });
        assert!(!gate.allows("heartbeat"));

        handle.set(GateState {
            paused: false,
            disabled: BTreeSet::from(["heartbeat".to_string()]),
        });
        assert!(!gate.allows("heartbeat"));
        assert!(gate.allows("files"));
    }
}
