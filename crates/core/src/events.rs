use hearth_protocol::Event;
use tokio::sync::broadcast;

/// Fan-out of daemon events to every connected client. Slow clients that fall more than
/// `CAPACITY` events behind get a `Resync` instead of the events they missed.
#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<Event>,
}

const CAPACITY: usize = 1024;

impl EventBus {
    pub fn new() -> Self {
        Self {
            tx: broadcast::channel(CAPACITY).0,
        }
    }

    pub fn publish(&self, event: Event) {
        // An error only means nobody is listening right now.
        let _ = self.tx.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.tx.subscribe()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}
