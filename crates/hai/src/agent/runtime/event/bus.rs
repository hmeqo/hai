use std::time::Duration;

use tokio::sync::mpsc;

pub use crate::domain::vo::{AgentEvent, AgentEventPayload};
use crate::domain::{repo::Repos, vo::ChatId};

// ── AgentEventBus ──────────────────────────────────────────────────────────────

const FLUSH_INTERVAL: Duration = Duration::from_millis(200);
const FLUSH_BATCH: usize = 50;

#[derive(Clone)]
pub struct AgentEventBus {
    tx: mpsc::UnboundedSender<AgentEvent>,
}

impl AgentEventBus {
    pub fn new(repos: Repos) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(collector(rx, repos));
        Self { tx }
    }

    pub fn emit(&self, chat_id: ChatId, payload: AgentEventPayload) {
        let _ = self.tx.send(AgentEvent { chat_id, payload });
    }
}

async fn collector(mut rx: mpsc::UnboundedReceiver<AgentEvent>, repos: Repos) {
    let mut batch = Vec::with_capacity(FLUSH_BATCH);
    loop {
        tokio::select! {
            Some(event) = rx.recv() => {
                batch.push(event);
                if batch.len() >= FLUSH_BATCH {
                    flush(&batch, &repos).await;
                    batch.clear();
                }
            }
            _ = tokio::time::sleep(FLUSH_INTERVAL) => {
                if !batch.is_empty() {
                    flush(&batch, &repos).await;
                    batch.clear();
                }
            }
            else => break,
        }
    }
}

async fn flush(batch: &[AgentEvent], repos: &Repos) {
    for event in batch {
        if let Err(e) = repos.event.insert("agent", event.to_json_value()).await {
            tracing::warn!(kind = event.payload.kind(), error = %e, "Failed to flush agent event");
        }
    }
}
