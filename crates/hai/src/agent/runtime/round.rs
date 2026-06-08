use autoagents::core::tool::ToolCallResult;
use tokio::task::JoinHandle;

use crate::agent::{AgentEngine, runtime::ctx::RoundCtx};

pub enum RoundResult {
    Completed(Round),
    Failed,
}

#[derive(Clone)]
pub struct Round {
    pub segment: String,
    pub tool_calls: Vec<ToolCallResult>,
    pub notes: Option<String>,
}

/// 一次 prepare 的产出，供 RoundTask 执行 LLM。
pub struct RoundTaskPayload {
    /// 发送给 LLM 的完整 prompt（含所有历史段）
    pub prompt: String,
    /// 本轮新增段（最终存入 Round）
    pub segment: String,
    /// 本轮涉及的消息 ID
    pub message_ids: Vec<i64>,
}

pub(crate) struct RoundTask {
    join_handle: JoinHandle<()>,
}

impl Drop for RoundTask {
    fn drop(&mut self) {
        self.join_handle.abort();
    }
}

impl RoundTask {
    pub(crate) fn spawn(
        engine: AgentEngine,
        ctx: RoundCtx,
        payload: RoundTaskPayload,
        on_complete: impl FnOnce(RoundResult) + Send + 'static,
    ) -> Self {
        let chat_id = ctx.chat_id;
        let raw_chat_id = chat_id.0;
        let heartbeat_bot = ctx.bot.clone();
        let events_reasons: Vec<&str> = ctx.events.iter().map(|e| e.reason.label()).collect();

        let handle = tokio::spawn(async move {
            tracing::info!(raw_chat_id, reasons = ?events_reasons, "Agent woke up");

            let RoundTaskPayload {
                prompt,
                segment,
                message_ids,
            } = payload;
            tracing::info!(raw_chat_id, "Agent task message:\n{prompt}");

            let heartbeat = async {
                let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
                loop {
                    interval.tick().await;
                    heartbeat_bot.send_typing(chat_id).await;
                }
            };
            let task = engine.run(&ctx, prompt);
            tokio::pin!(heartbeat);
            tokio::pin!(task);

            let output = tokio::select! {
                r = &mut task => match r {
                    Ok(output) => {
                        tracing::info!(raw_chat_id, ?output, "Agent done");
                        Some(output)
                    }
                    Err(err) => {
                        tracing::error!(raw_chat_id, "Agent run failed: {err}");
                        None
                    }
                },
                _ = &mut heartbeat => unreachable!(),
            };

            match output {
                Some(output) => {
                    if !message_ids.is_empty()
                        && let Err(err) =
                            ctx.app.db.srv.message.mark_unread_seen(&message_ids).await
                    {
                        tracing::warn!(raw_chat_id, "Failed to mark messages seen: {err}");
                    }

                    on_complete(RoundResult::Completed(Round {
                        segment,
                        tool_calls: output.tool_calls,
                        notes: output.notes,
                    }));
                }
                None => {
                    on_complete(RoundResult::Failed);
                }
            }
        });

        Self {
            join_handle: handle,
        }
    }
}
