use std::time::Duration;

use tokio::sync::mpsc;

use crate::domain::vo::ChatId;
pub use crate::domain::vo::{AgentEvent, AgentEventPayload};

// ── AgentEventBus ──────────────────────────────────────────────────────────────

const FLUSH_INTERVAL: Duration = Duration::from_millis(200);
const FLUSH_BATCH: usize = 50;

#[derive(Clone)]
pub struct AgentEventBus {
    tx: mpsc::UnboundedSender<AgentEvent>,
}

impl AgentEventBus {
    pub fn new(db: toasty::Db) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(collector(rx, db));
        Self { tx }
    }

    pub fn emit(&self, chat_id: ChatId, payload: AgentEventPayload) {
        let _ = self.tx.send(AgentEvent { chat_id, payload });
    }
}

async fn collector(mut rx: mpsc::UnboundedReceiver<AgentEvent>, db: toasty::Db) {
    let mut batch = Vec::with_capacity(FLUSH_BATCH);
    loop {
        tokio::select! {
            Some(event) = rx.recv() => {
                batch.push(event);
                if batch.len() >= FLUSH_BATCH {
                    flush(&batch, &db).await;
                    batch.clear();
                }
            }
            _ = tokio::time::sleep(FLUSH_INTERVAL) => {
                if !batch.is_empty() {
                    flush(&batch, &db).await;
                    batch.clear();
                }
            }
            else => break,
        }
    }
}

async fn flush(batch: &[AgentEvent], db: &toasty::Db) {
    use crate::domain::model::Event;

    for event in batch {
        if let Err(e) = toasty::create!(Event {
            domain: "agent".to_string(),
            payload: toasty::Json(event.to_json_value()),
        })
        .exec(&mut db.clone())
        .await
        {
            tracing::warn!(kind = event.payload.kind(), error = %e, "Failed to flush agent event");
        }
    }
}
