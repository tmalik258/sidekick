use std::time::Duration;

use sidekick_core::{Event, EventBus};
use tokio::task::JoinHandle;

use crate::{Sensor, SensorGate};

/// Emits `debug.heartbeat` on a fixed interval. P0 uses it to prove the
/// sensor, bus, storage, and mascot pipeline end to end.
pub struct HeartbeatSensor {
    interval: Duration,
}

impl HeartbeatSensor {
    pub const ID: &'static str = "heartbeat";
    pub const EVENT_KIND: &'static str = "debug.heartbeat";

    pub fn new(interval: Duration) -> Self {
        Self { interval }
    }
}

impl Sensor for HeartbeatSensor {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn spawn(self: Box<Self>, bus: EventBus, gate: SensorGate) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(self.interval);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            // The first tick fires immediately; skip it so the app starts quietly.
            ticker.tick().await;
            let mut beat: u64 = 0;
            loop {
                ticker.tick().await;
                if !gate.allows(Self::ID) {
                    continue;
                }
                beat += 1;
                bus.publish(Event::new(
                    Self::EVENT_KIND,
                    Self::ID,
                    serde_json::json!({ "beat": beat }),
                ));
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GateState;

    #[tokio::test(start_paused = true)]
    async fn emits_only_while_the_gate_allows() {
        let bus = EventBus::default();
        let mut rx = bus.subscribe();
        let (handle, gate) = SensorGate::new(GateState::default());
        let task = Box::new(HeartbeatSensor::new(Duration::from_secs(30))).spawn(bus.clone(), gate);

        tokio::time::sleep(Duration::from_secs(31)).await;
        let event = rx.recv().await.unwrap();
        assert_eq!(event.kind, HeartbeatSensor::EVENT_KIND);
        assert_eq!(event.payload["beat"], 1);

        handle.set(GateState {
            paused: true,
            ..GateState::default()
        });
        tokio::time::sleep(Duration::from_secs(95)).await;
        assert!(rx.try_recv().is_err());

        task.abort();
    }
}
