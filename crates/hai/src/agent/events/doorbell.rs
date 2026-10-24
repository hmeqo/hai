use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use tokio::sync::Notify;

use crate::domain::vo::ChatId;

/// A doorbell for one Conversation: the changes themselves live in the database. `Notify` coalesces
/// wake-ups; each origin Chat's arrival version is what a Turn compares against.
#[derive(Clone)]
pub struct Doorbell {
    notify: Arc<Notify>,
    versions: Arc<Mutex<HashMap<ChatId, u64>>>,
}

impl Doorbell {
    pub fn new() -> Self {
        Self {
            notify: Arc::new(Notify::new()),
            versions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Wakes the loop without attributing the arrival to a Chat.
    pub fn notify(&self) {
        self.notify.notify_one();
    }

    /// Wakes the loop and advances this origin Chat's arrival version.
    pub fn notify_chat(&self, chat_id: ChatId) {
        let mut versions = self.versions.lock().unwrap_or_else(|e| e.into_inner());
        let version = versions.entry(chat_id).or_default();
        *version = version.wrapping_add(1);
        drop(versions);
        self.notify();
    }

    pub fn notified(&self) -> impl Future<Output = ()> + '_ {
        self.notify.notified()
    }

    /// The current arrival version of one Chat.
    pub fn version(&self, chat_id: ChatId) -> u64 {
        self.versions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&chat_id)
            .copied()
            .unwrap_or_default()
    }

    /// Whether this Chat has arrived since the Turn recorded `since`.
    pub fn steered(&self, chat_id: ChatId, since: u64) -> bool {
        self.version(chat_id) != since
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Chat other than the origin must not interrupt the running Turn.
    #[test]
    fn steering_is_per_origin_chat() {
        let doorbell = Doorbell::new();
        let since = doorbell.version(ChatId(1));
        doorbell.notify_chat(ChatId(2));
        assert!(!doorbell.steered(ChatId(1), since));
        assert!(doorbell.steered(ChatId(2), 0));
    }

    /// Arrival versions survive a wake-up; only the comparison against the Turn's start matters.
    #[test]
    fn a_version_advances_once_per_arrival() {
        let doorbell = Doorbell::new();
        let since = doorbell.version(ChatId(3));
        doorbell.notify_chat(ChatId(3));
        doorbell.notify_chat(ChatId(3));
        assert_eq!(doorbell.version(ChatId(3)), since + 2);
        assert!(doorbell.steered(ChatId(3), since));
    }
}
