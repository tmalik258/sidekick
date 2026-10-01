//! Sensors turn OS signals into [`Event`]s on the [`EventBus`].
//!
//! Each sensor runs as its own tokio task and checks a [`SensorGate`] before
//! emitting, so pausing (FR-SET-01) and per-sensor switches (FR-SEN-12) are
//! enforced in one place.

mod browser;
pub mod calendar;
pub mod classify;
mod claude_code;
mod clipboard;
mod downloads;
mod gate;
mod heartbeat;
pub mod http;
mod idle;
mod ports;
pub mod repos;
mod system;
mod window;

pub use browser::{BrowserBridge, BrowserSensor};
pub use calendar::{Calendar, CalendarSensor};
pub use claude_code::ClaudeCodeSensor;
pub use clipboard::ClipboardSensor;
pub use downloads::DownloadsSensor;
pub use gate::{GateState, SensorGate, SensorGateHandle};
pub use heartbeat::HeartbeatSensor;
pub use idle::IdleSensor;
pub use ports::PortsSensor;
pub use repos::ReposSensor;
pub use system::SystemSensor;
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
