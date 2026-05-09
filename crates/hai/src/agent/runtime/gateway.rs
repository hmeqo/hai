use std::{
    collections::HashMap,
    sync::Arc,
};

use tokio::sync::mpsc;

use crate::agent::{
    event::WakeEvent,
    link::{AgentLink, BotConn, BotId},
    runtime::AgentCtx,
};
use crate::error::Result;

/// Agent 事件网关
///
/// 负责连接管理（`add_connection`）和事件分发（`run`）。
/// 执行委托给 `AgentCtx`（`Arc` 共享）。
pub struct AgentGateway {
    ctx: Arc<AgentCtx>,
    conns: HashMap<BotId, BotConn>,
    links: HashMap<BotId, AgentLink>,
}

impl AgentGateway {
    pub fn new(ctx: Arc<AgentCtx>) -> Self {
        Self { ctx, conns: HashMap::new(), links: HashMap::new() }
    }

    /// 注册一个 bot 连接
    pub fn add_connection(&mut self, bot_id: BotId, conn: BotConn, link: AgentLink) {
        self.conns.insert(bot_id.clone(), conn);
        self.links.insert(bot_id, link);
    }

    /// 事件路由：不同 chat 并行，同一 chat 串行
    pub async fn run(mut self) -> Result<()> {
        let (merged_tx, mut merged_rx) = mpsc::unbounded_channel();

        for (bot_id, mut link) in self.links.drain() {
            let tx = merged_tx.clone();
            tokio::spawn(async move {
                while let Some(event) = link.event_rx.recv().await {
                    if let Err(err) = tx.send((bot_id.clone(), event)) {
                        tracing::error!("Failed to send event to gateway: {err}");
                    }
                }
            });
        }
        drop(merged_tx);

        let mut sessions: HashMap<i64, mpsc::UnboundedSender<WakeEvent>> = HashMap::new();

        while let Some((bot_id, event)) = merged_rx.recv().await {
            let chat_id = event.chat_id;

            if sessions.get(&chat_id).is_none_or(|tx| tx.is_closed()) {
                let conn = self
                    .conns
                    .get(&bot_id)
                    .cloned()
                    .expect("BotConn must exist for connected bot");
                sessions.insert(
                    chat_id,
                    super::session::spawn_chat_session(Arc::clone(&self.ctx), chat_id, conn),
                );
            }

            if let Err(err) = sessions[&chat_id].send(event) {
                tracing::error!(chat_id, "Failed to send event to chat session: {err}");
            }
        }
        Ok(())
    }
}
