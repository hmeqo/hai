use tokio::time::Duration;

use super::AgentSession;
use crate::domain::model::Message;

impl AgentSession {
    pub(super) async fn gather_messages(&self) -> (Vec<Message>, i64) {
        let cfg = &self.engine.app.cfg.agent.context;
        let msg_srv = &self.engine.app.db.srv.message;

        if let Some(conv) = &self.conversation
            && let Some(last) = conv.last_run()
        {
            let since_id = last.since_id;
            match msg_srv
                .get_messages_window(self.chat_id, Some(since_id), cfg.history_cap)
                .await
            {
                Ok(msgs) => {
                    let next_id = msgs.last().map(|m| m.id).unwrap_or(since_id);
                    (msgs, next_id)
                }
                Err(e) => {
                    tracing::error!(%self.chat_id, "Failed to gather messages: {e}");
                    (Vec::new(), since_id)
                }
            }
        } else {
            match msg_srv
                .get_context_messages(self.chat_id, cfg.history_cap, cfg.message_history_limit)
                .await
            {
                Ok((msgs, last_id)) => (msgs, last_id),
                Err(e) => {
                    tracing::error!(%self.chat_id, "Failed to fetch context messages: {e}");
                    (Vec::new(), -1)
                }
            }
        }
    }

    pub(super) fn idle_timeout(&self) -> Duration {
        Duration::from_secs(self.engine.app.cfg.agent.context.session_idle_timeout_secs)
    }
}
