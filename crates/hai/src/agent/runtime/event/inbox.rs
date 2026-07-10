use std::sync::{Arc, Mutex};

use tokio::sync::Notify;

use super::wake::{WakeEvent, WakeEvents};

#[derive(Clone)]
pub struct Inbox {
    events: Arc<Mutex<Vec<WakeEvent>>>,
    notify: Arc<Notify>,
}

impl Inbox {
    pub fn new() -> Self {
        Self {
            events: Arc::new(Mutex::new(Vec::new())),
            notify: Arc::new(Notify::new()),
        }
    }

    pub fn push(&self, event: WakeEvent) {
        self.events.lock().unwrap().push(event);
        self.notify.notify_one();
    }

    pub fn drain(&self) -> WakeEvents {
        WakeEvents::new(std::mem::take(&mut *self.events.lock().unwrap()))
    }

    pub fn notified(&self) -> impl std::future::Future<Output = ()> + '_ {
        self.notify.notified()
    }

    pub fn len(&self) -> usize {
        self.events.lock().unwrap().len()
    }
}
