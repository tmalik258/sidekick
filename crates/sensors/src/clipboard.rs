use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::Duration;

use sidekick_core::{Event, EventBus, Sensitivity};
use tokio::task::JoinHandle;

use crate::classify::{ClipKind, clip_kind};
use crate::{Sensor, SensorGate};

/// Notices copied text and classifies it (FR-SEN-06). Secrets are flagged
/// and their text never leaves this sensor (FR-CLIP-02).
pub struct ClipboardSensor;

impl ClipboardSensor {
    pub const ID: &'static str = "clipboard";
    pub const EVENT_KIND: &'static str = "clipboard.changed";
}

const CHECK_EVERY: Duration = Duration::from_millis(300);
/// Longer text is truncated in the event payload.
const MAX_TEXT: usize = 20_000;

impl Sensor for ClipboardSensor {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn spawn(self: Box<Self>, bus: EventBus, gate: SensorGate) -> JoinHandle<()> {
        // The clipboard handle is not Send on every platform, so it lives on
        // its own thread and reports through the bus.
        let handle = tokio::runtime::Handle::current();
        let thread = std::thread::Builder::new()
            .name("sidekick-clipboard".into())
            .spawn(move || {
                let mut clipboard = match arboard::Clipboard::new() {
                    Ok(c) => c,
                    Err(err) => {
                        log::warn!("clipboard sensor unavailable: {err}");
                        return;
                    }
                };
                // Whatever is on the clipboard at start is not news.
                let mut last = clipboard.get_text().ok().map(|t| digest(&t));
                loop {
                    std::thread::sleep(CHECK_EVERY);
                    let Ok(text) = clipboard.get_text() else {
                        continue;
                    };
                    let d = digest(&text);
                    if last == Some(d) || text.trim().is_empty() {
                        continue;
                    }
                    last = Some(d);
                    if gate.allows(Self::ID) {
                        bus.publish(clip_event(&text));
                    }
                }
            });
        handle.spawn(async move {
            if let Err(err) = thread {
                log::error!("clipboard sensor thread failed to start: {err}");
            }
        })
    }
}

fn digest(text: &str) -> u64 {
    let mut h = DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

fn clip_event(text: &str) -> Event {
    let kind = clip_kind(text);
    let trimmed = text.trim();
    let preview: String = trimmed.chars().take(80).collect();
    let payload = if kind == ClipKind::Secret {
        serde_json::json!({ "kind": kind.as_str(), "length": trimmed.chars().count() })
    } else {
        let body: String = trimmed.chars().take(MAX_TEXT).collect();
        let mut payload = serde_json::json!({
            "kind": kind.as_str(),
            "length": trimmed.chars().count(),
            "preview": preview,
            "text": body,
        });
        if kind == ClipKind::Color
            && let (Some(obj), Some(serde_json::Value::Object(extra))) =
                (payload.as_object_mut(), crate::color::info(trimmed))
        {
            obj.extend(extra);
        }
        payload
    };
    let sensitivity = if kind == ClipKind::Secret {
        Sensitivity::Secret
    } else {
        Sensitivity::Personal
    };
    Event::new(ClipboardSensor::EVENT_KIND, ClipboardSensor::ID, payload)
        .with_sensitivity(sensitivity)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_events_carry_no_text() {
        let e = clip_event("API_KEY=abcdef123456");
        assert_eq!(e.sensitivity, Sensitivity::Secret);
        assert!(e.payload.get("text").is_none());
        assert!(e.payload.get("preview").is_none());
    }

    #[test]
    fn regular_events_carry_text_and_preview() {
        let e = clip_event("https://example.com/a");
        assert_eq!(e.payload["kind"], "url");
        assert_eq!(e.payload["text"], "https://example.com/a");
    }
}
