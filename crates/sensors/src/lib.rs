//! Sensors turn OS signals into [`Event`]s on the [`EventBus`].
//!
//! Each sensor runs as its own tokio task and checks a [`SensorGate`] before
//! emitting, so pausing (FR-SET-01) and per-sensor switches (FR-SEN-12) are
//! enforced in one place.

pub mod classify;
mod clipboard;
mod downloads;
mod gate;
mod heartbeat;
mod ports;
mod window;

pub use clipboard::ClipboardSensor;
pub use downloads::DownloadsSensor;
pub use gate::{GateState, SensorGate, SensorGateHandle};
pub use heartbeat::HeartbeatSensor;
pub use ports::PortsSensor;
pub use window::WindowSensor;

use sidekick_core::EventBus;
use tokio::task::JoinHandle;

pub trait Sensor: Send + 'static {
    /// Stable id used in settings and as the event `source`.
    fn id(&self) -> &'static str;

    /// Starts the sensor on the current tokio runtime.
    fn spawn(self: Box<Self>, bus: EventBus, gate: SensorGate) -> JoinHandle<()>;
}

/// Starts every sensor and returns their task handles.
pub fn spawn_all(
    sensors: Vec<Box<dyn Sensor>>,
    bus: &EventBus,
    gate: &SensorGate,
) -> Vec<JoinHandle<()>> {
    sensors
        .into_iter()
        .map(|sensor| {
            log::info!("starting sensor {}", sensor.id());
            sensor.spawn(bus.clone(), gate.clone())
        })
        .collect()
}
