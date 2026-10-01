use tokio::sync::broadcast;

use crate::event::Event;

const DEFAULT_CAPACITY: usize = 1024;

/// In-process fan-out of events. Cheap to clone; every clone publishes to the
/// same subscribers.
#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<Event>,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }

    /// Publishes an event. Returns how many subscribers received it; zero is
    /// not an error because subscribers may not be running yet.
    pub fn publish(&self, event: Event) -> usize {
        self.tx.send(event).unwrap_or(0)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.tx.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn delivers_to_every_subscriber() {
        let bus = EventBus::default();
        let mut a = bus.subscribe();
        let mut b = bus.subscribe();
        let sent = bus.publish(Event::new("test.ping", "test", serde_json::Value::Null));
        assert_eq!(sent, 2);
        assert_eq!(a.recv().await.unwrap().kind, "test.ping");
        assert_eq!(b.recv().await.unwrap().kind, "test.ping");
    }

    #[test]
    fn publishing_without_subscribers_is_fine() {
        let bus = EventBus::default();
        assert_eq!(
            bus.publish(Event::new("test.ping", "test", serde_json::Value::Null)),
            0
        );
    }
}
