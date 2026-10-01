use std::time::Duration;

use sidekick_core::{Event, EventBus, Sensitivity};
use tokio::task::JoinHandle;

use crate::{Sensor, SensorGate};

/// Notices which app and window the user is in (FR-SEN-05).
pub struct WindowSensor;

impl WindowSensor {
    pub const ID: &'static str = "window";
    pub const EVENT_KIND: &'static str = "window.focused";
}

const CHECK_EVERY: Duration = Duration::from_millis(500);

impl Sensor for WindowSensor {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn spawn(self: Box<Self>, bus: EventBus, gate: SensorGate) -> JoinHandle<()> {
        tokio::spawn(async move {
            let own_pid = u64::from(std::process::id());
            let mut last: Option<(String, String)> = None;
            let mut tick = tokio::time::interval(CHECK_EVERY);
            loop {
                tick.tick().await;
                let Ok(Ok(win)) =
                    tokio::task::spawn_blocking(active_win_pos_rs::get_active_window).await
                else {
                    continue;
                };
                if win.process_id == own_pid {
                    continue;
                }
                let key = (win.app_name.clone(), win.title.clone());
                if last.as_ref() == Some(&key) {
                    continue;
                }
                last = Some(key);
                if !gate.allows(Self::ID) {
                    continue;
                }
                let exe = win
                    .process_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                bus.publish(
                    Event::new(
                        Self::EVENT_KIND,
                        Self::ID,
                        serde_json::json!({
                            "app": win.app_name,
                            "exe": exe,
                            "title": win.title,
                            "pid": win.process_id,
                            "x": win.position.x,
                            "y": win.position.y,
                            "width": win.position.width,
                            "height": win.position.height,
                        }),
                    )
                    .with_sensitivity(Sensitivity::Personal),
                );
            }
        })
    }
}
